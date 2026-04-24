use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{
    domain::{
        AssetHashFields, AssetMetadata, AssetRecord, AssetSourceContext, ExtractedAssetRecord,
        PreparedAssetInventory, PreparedAssetInventoryContext,
    },
    error::{AppError, AppResult},
};

const FRAME_ANALYSIS_TOOL: &str = "3dmigoto-frame-analysis";
const FRAME_ANALYSIS_KIND: &str = "runtime_draw_call_hashes";
const RUNTIME_DRAW_CALL_KIND: &str = "runtime_draw_call";
const PREPARED_ASSET_INVENTORY_SCHEMA: &str = "whashreonator.prepared-assets.v1";
const DEFAULT_BINDING_SLOT: &str = "0";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameAnalysisDump {
    pub dump_dir: PathBuf,
    pub log_path: PathBuf,
    pub options_header: String,
    pub draw_calls: Vec<FrameAnalysisDrawCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameAnalysisDrawCall {
    pub drawcall: u32,
    pub vb_bindings: Vec<FrameAnalysisBinding>,
    pub ib_binding: Option<FrameAnalysisBinding>,
    pub vs_binding: Option<FrameAnalysisBinding>,
    pub ps_binding: Option<FrameAnalysisBinding>,
    /// Compute shader binding for Dispatch draw calls. Captured so the
    /// inventory can surface compute-shader hashes, which are one of the
    /// WWMI plugin's hardcoded hook anchors (e.g. ShapeKeyLoaderCS).
    pub cs_binding: Option<FrameAnalysisBinding>,
    /// Textures bound to the pixel shader for this draw call. Captured so
    /// the inventory can surface UI texture anchors that the WWMI plugin
    /// relies on for menu/dressing-room detection.
    pub ps_texture_bindings: Vec<FrameAnalysisBinding>,
    pub draw: Option<FrameAnalysisDraw>,
    ib_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameAnalysisBinding {
    pub slot: String,
    pub view_address: Option<String>,
    pub resource_address: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameAnalysisDraw {
    Indexed {
        index_count: u32,
        start_index: u32,
        base_vertex: i32,
    },
    NonIndexed {
        vertex_count: u32,
        start_vertex: u32,
    },
    /// `Dispatch` / `DispatchIndirect` — compute-shader invocation. Thread
    /// group counts come from the API line; indirect dispatches have
    /// unknown counts and store zeros.
    Compute {
        thread_group_x: u32,
        thread_group_y: u32,
        thread_group_z: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingBindingKind {
    VertexBuffer,
    IndexBuffer,
    VertexShader,
    PixelShader,
    PixelShaderTextures,
    Ignore,
}

#[derive(Debug, Default)]
struct IbAggregate {
    draw_calls: BTreeSet<u32>,
    max_index_count: Option<u32>,
    index_format: Option<String>,
}

#[derive(Debug, Default)]
struct VbAggregate {
    draw_calls: BTreeSet<u32>,
    slots: BTreeSet<String>,
    max_vertex_count: Option<u32>,
    vs_hashes: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct ShaderAggregate {
    draw_calls: BTreeSet<u32>,
}

#[derive(Debug, Default)]
struct CsAggregate {
    draw_calls: BTreeSet<u32>,
    thread_group_dims: BTreeSet<(u32, u32, u32)>,
}

#[derive(Debug, Default)]
struct PsTextureAggregate {
    draw_calls: BTreeSet<u32>,
    slots: BTreeSet<String>,
    ps_hashes: BTreeSet<String>,
}

impl FrameAnalysisDrawCall {
    fn is_meaningful(&self) -> bool {
        !self.vb_bindings.is_empty()
            || self.ib_binding.is_some()
            || self.vs_binding.is_some()
            || self.ps_binding.is_some()
            || self.cs_binding.is_some()
            || !self.ps_texture_bindings.is_empty()
            || self.draw.is_some()
    }

    fn upsert_vb_binding(&mut self, binding: FrameAnalysisBinding) {
        if let Some(existing) = self
            .vb_bindings
            .iter_mut()
            .find(|candidate| candidate.slot == binding.slot)
        {
            *existing = binding;
            return;
        }

        self.vb_bindings.push(binding);
        self.vb_bindings
            .sort_by(|left, right| left.slot.cmp(&right.slot));
    }

    /// Proxy count used to aggregate both IB `index_count` and VB `vertex_count`
    /// fields. For `DrawIndexed` there is no direct `VertexCount`, so
    /// `IndexCount` is reused as the best available hint for both sides;
    /// for `Draw`, `VertexCount` is authoritative.
    fn draw_count_hint(&self) -> Option<u32> {
        match self.draw.as_ref() {
            Some(FrameAnalysisDraw::Indexed { index_count, .. }) => Some(*index_count),
            Some(FrameAnalysisDraw::NonIndexed { vertex_count, .. }) => Some(*vertex_count),
            Some(FrameAnalysisDraw::Compute { .. }) => None,
            None => None,
        }
    }
}

pub fn parse_frame_analysis_log(text: &str) -> AppResult<FrameAnalysisDump> {
    let mut lines = text.lines();
    let options_header = lines
        .next()
        .ok_or_else(|| AppError::InvalidInput("frame analysis log is empty".to_string()))?
        .trim_end()
        .to_string();
    // 3DMigoto fixture style writes `analyse_options=<flag list>`; live WWMI
    // dumps write `analyse_options: <hex flags>`. Accept either form.
    if !options_header.starts_with("analyse_options=")
        && !options_header.starts_with("analyse_options:")
    {
        return Err(AppError::InvalidInput(
            "frame analysis log must start with analyse_options=... or analyse_options: ..."
                .to_string(),
        ));
    }

    let mut draw_calls = Vec::new();
    let mut current_draw_call = None::<FrameAnalysisDrawCall>;
    let mut pending_binding_kind = None::<PendingBindingKind>;

    for raw_line in lines {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            pending_binding_kind = None;
            continue;
        }

        if let Some((drawcall, api_call)) = parse_drawcall_line(line)? {
            if current_draw_call
                .as_ref()
                .is_some_and(|current| current.drawcall != drawcall)
            {
                finalize_draw_call(&mut draw_calls, current_draw_call.take());
            }

            let draw_call = current_draw_call.get_or_insert_with(|| FrameAnalysisDrawCall {
                drawcall,
                ..FrameAnalysisDrawCall::default()
            });
            if draw_call.drawcall != drawcall {
                return Err(AppError::InvalidInput(format!(
                    "frame analysis drawcall stream became inconsistent around line: {line}"
                )));
            }

            pending_binding_kind = apply_api_call(draw_call, api_call)?;
            continue;
        }

        if !raw_line.starts_with(char::is_whitespace) {
            return Err(AppError::InvalidInput(format!(
                "unrecognized frame analysis log line: {line}"
            )));
        }

        let Some(kind) = pending_binding_kind else {
            continue;
        };
        if kind == PendingBindingKind::Ignore {
            continue;
        }

        let draw_call = current_draw_call.as_mut().ok_or_else(|| {
            AppError::InvalidInput("binding line encountered before any drawcall line".to_string())
        })?;
        let binding = parse_binding_line(line, kind)?;
        match kind {
            PendingBindingKind::VertexBuffer => draw_call.upsert_vb_binding(binding),
            PendingBindingKind::IndexBuffer => draw_call.ib_binding = Some(binding),
            PendingBindingKind::VertexShader => draw_call.vs_binding = Some(binding),
            PendingBindingKind::PixelShader => draw_call.ps_binding = Some(binding),
            PendingBindingKind::PixelShaderTextures => {
                draw_call.ps_texture_bindings.push(binding);
            }
            PendingBindingKind::Ignore => {}
        }
    }

    finalize_draw_call(&mut draw_calls, current_draw_call.take());

    Ok(FrameAnalysisDump {
        dump_dir: PathBuf::new(),
        log_path: PathBuf::new(),
        options_header,
        draw_calls,
    })
}

pub fn build_prepared_inventory(
    dump: &FrameAnalysisDump,
    version_id: &str,
) -> PreparedAssetInventory {
    let dump_root = normalize_optional_path(&dump.dump_dir);
    let note = format!(
        "Captured from {}; {} draw calls, {} unique IB hashes, {} unique VB hashes",
        if dump.log_path.as_os_str().is_empty() {
            "FrameAnalysis/log.txt".to_string()
        } else {
            dump.log_path.to_string_lossy().replace('\\', "/")
        },
        dump.draw_calls.len(),
        unique_ib_hash_count(dump),
        unique_vb_hash_count(dump)
    );

    let mut ib_assets = BTreeMap::<String, IbAggregate>::new();
    let mut vb_assets = BTreeMap::<String, VbAggregate>::new();
    let mut vs_assets = BTreeMap::<String, ShaderAggregate>::new();
    let mut ps_assets = BTreeMap::<String, ShaderAggregate>::new();
    let mut cs_assets = BTreeMap::<String, CsAggregate>::new();
    // Textures bound to PS are keyed by (ps_hash, slot) so the identity
    // tuple stays stable across a texture content hash drift as long as the
    // parent pixel shader and slot position do not change.
    let mut ps_texture_assets = BTreeMap::<String, PsTextureAggregate>::new();

    for draw_call in &dump.draw_calls {
        let count_hint = draw_call.draw_count_hint();

        if let Some(binding) = draw_call.ib_binding.as_ref() {
            let aggregate = ib_assets.entry(binding.hash.clone()).or_default();
            aggregate.draw_calls.insert(draw_call.drawcall);
            if let Some(index_count) = count_hint {
                aggregate.max_index_count =
                    Some(aggregate.max_index_count.unwrap_or(0).max(index_count));
            }
            if aggregate.index_format.is_none() {
                aggregate.index_format = draw_call.ib_format.clone();
            }
        }

        if let Some(binding) = draw_call.vs_binding.as_ref() {
            vs_assets
                .entry(binding.hash.clone())
                .or_default()
                .draw_calls
                .insert(draw_call.drawcall);
        }

        if let Some(binding) = draw_call.ps_binding.as_ref() {
            ps_assets
                .entry(binding.hash.clone())
                .or_default()
                .draw_calls
                .insert(draw_call.drawcall);
        }

        if let Some(binding) = draw_call.cs_binding.as_ref() {
            let aggregate = cs_assets.entry(binding.hash.clone()).or_default();
            aggregate.draw_calls.insert(draw_call.drawcall);
            if let Some(FrameAnalysisDraw::Compute {
                thread_group_x,
                thread_group_y,
                thread_group_z,
            }) = draw_call.draw.as_ref()
            {
                aggregate.thread_group_dims.insert((
                    *thread_group_x,
                    *thread_group_y,
                    *thread_group_z,
                ));
            }
        }

        for binding in &draw_call.vb_bindings {
            let aggregate = vb_assets.entry(binding.hash.clone()).or_default();
            aggregate.draw_calls.insert(draw_call.drawcall);
            aggregate.slots.insert(binding.slot.clone());
            if let Some(vertex_count) = count_hint {
                aggregate.max_vertex_count =
                    Some(aggregate.max_vertex_count.unwrap_or(0).max(vertex_count));
            }
            if let Some(vs_binding) = draw_call.vs_binding.as_ref() {
                aggregate.vs_hashes.insert(vs_binding.hash.clone());
            }
        }

        for binding in &draw_call.ps_texture_bindings {
            let aggregate = ps_texture_assets.entry(binding.hash.clone()).or_default();
            aggregate.draw_calls.insert(draw_call.drawcall);
            aggregate.slots.insert(binding.slot.clone());
            if let Some(ps_binding) = draw_call.ps_binding.as_ref() {
                aggregate.ps_hashes.insert(ps_binding.hash.clone());
            }
        }
    }

    let mut assets = Vec::<ExtractedAssetRecord>::new();

    for (hash, aggregate) in ib_assets {
        let index_format = aggregate.index_format.clone();
        let identity_tuple = Some(format!(
            "fa|ib|idx_fmt:{}|idx_count:{}",
            index_format.as_deref().unwrap_or("none"),
            aggregate.max_index_count.unwrap_or(0)
        ));
        assets.push(ExtractedAssetRecord {
            asset: AssetRecord {
                id: format!("ib_{hash}"),
                path: format!("runtime/ib/{hash}"),
                kind: Some("index_buffer".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some(format!("ib_{hash}")),
                    index_count: aggregate.max_index_count,
                    index_format,
                    tags: vec![format!("draw_calls={}", aggregate.draw_calls.len())],
                    ..AssetMetadata::default()
                },
            },
            hash_fields: AssetHashFields {
                asset_hash: Some(hash),
                shader_hash: None,
                signature: None,
                identity_tuple,
            },
            source: frame_analysis_source_context(dump_root.clone()),
        });
    }

    for (hash, aggregate) in vb_assets {
        let shader_hash = if aggregate.vs_hashes.len() == 1 {
            aggregate.vs_hashes.into_iter().next()
        } else {
            None
        };
        let identity_tuple = shader_identity_tuple(shader_hash.as_deref());
        assets.push(ExtractedAssetRecord {
            asset: AssetRecord {
                id: format!("vb_{hash}"),
                path: format!("runtime/vb/{hash}"),
                kind: Some("vertex_buffer".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some(format!("vb_{hash}")),
                    vertex_count: aggregate.max_vertex_count,
                    vertex_buffer_count: Some(aggregate.slots.len() as u32),
                    tags: vec![format!("draw_calls={}", aggregate.draw_calls.len())],
                    ..AssetMetadata::default()
                },
            },
            hash_fields: AssetHashFields {
                asset_hash: Some(hash),
                shader_hash,
                signature: None,
                identity_tuple,
            },
            source: frame_analysis_source_context(dump_root.clone()),
        });
    }

    for (hash, aggregate) in vs_assets {
        assets.push(build_shader_asset(
            "vs",
            "vertex_shader",
            hash,
            aggregate.draw_calls.len(),
            dump_root.clone(),
        ));
    }

    for (hash, aggregate) in ps_assets {
        assets.push(build_shader_asset(
            "ps",
            "pixel_shader",
            hash,
            aggregate.draw_calls.len(),
            dump_root.clone(),
        ));
    }

    for (hash, aggregate) in cs_assets {
        let identity_tuple = compute_shader_identity_tuple(&aggregate);
        assets.push(ExtractedAssetRecord {
            asset: AssetRecord {
                id: format!("cs_{hash}"),
                path: format!("runtime/cs/{hash}"),
                kind: Some("compute_shader".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some(format!("cs_{hash}")),
                    tags: vec![
                        format!("draw_calls={}", aggregate.draw_calls.len()),
                        "wwmi-anchor-candidate".to_string(),
                    ],
                    ..AssetMetadata::default()
                },
            },
            hash_fields: AssetHashFields {
                asset_hash: Some(hash),
                shader_hash: None,
                signature: None,
                identity_tuple,
            },
            source: frame_analysis_source_context(dump_root.clone()),
        });
    }

    for (hash, aggregate) in ps_texture_assets {
        let ps_hash = if aggregate.ps_hashes.len() == 1 {
            aggregate.ps_hashes.iter().next().cloned()
        } else {
            None
        };
        let slot = if aggregate.slots.len() == 1 {
            aggregate.slots.iter().next().cloned()
        } else {
            None
        };
        let identity_tuple = ps_texture_identity_tuple(ps_hash.as_deref(), slot.as_deref());
        let mut tags = vec![
            format!("draw_calls={}", aggregate.draw_calls.len()),
            "wwmi-anchor-candidate".to_string(),
        ];
        if let Some(slot_value) = slot.as_deref() {
            tags.push(format!("ps_slot={slot_value}"));
        }
        assets.push(ExtractedAssetRecord {
            asset: AssetRecord {
                id: format!("ps_tex_{hash}"),
                path: format!("runtime/ps_tex/{hash}"),
                kind: Some("texture_resource".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some(format!("ps_tex_{hash}")),
                    tags,
                    ..AssetMetadata::default()
                },
            },
            hash_fields: AssetHashFields {
                asset_hash: Some(hash),
                shader_hash: ps_hash,
                signature: None,
                identity_tuple,
            },
            source: frame_analysis_source_context(dump_root.clone()),
        });
    }

    assets.sort_by(|left, right| {
        left.asset
            .path
            .cmp(&right.asset.path)
            .then_with(|| left.asset.id.cmp(&right.asset.id))
    });

    PreparedAssetInventory {
        schema_version: PREPARED_ASSET_INVENTORY_SCHEMA.to_string(),
        context: PreparedAssetInventoryContext {
            extraction_tool: Some(FRAME_ANALYSIS_TOOL.to_string()),
            extraction_kind: Some(FRAME_ANALYSIS_KIND.to_string()),
            source_root: dump_root,
            version_id: Some(version_id.to_string()),
            tags: vec!["frame-analysis".to_string(), "wwmi".to_string()],
            meaningful_content_coverage: Some(true),
            meaningful_character_coverage: Some(true),
            note: Some(note),
        },
        assets,
    }
}

fn finalize_draw_call(
    draw_calls: &mut Vec<FrameAnalysisDrawCall>,
    draw_call: Option<FrameAnalysisDrawCall>,
) {
    if let Some(draw_call) = draw_call
        && draw_call.is_meaningful()
    {
        draw_calls.push(draw_call);
    }
}

fn parse_drawcall_line(line: &str) -> AppResult<Option<(u32, &str)>> {
    if !line
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        return Ok(None);
    }
    let Some((drawcall, remainder)) = line.split_once(' ') else {
        return Ok(None);
    };
    if !drawcall.chars().all(|character| character.is_ascii_digit()) {
        return Ok(None);
    }

    let drawcall = drawcall.parse::<u32>().map_err(|error| {
        AppError::InvalidInput(format!(
            "invalid frame analysis drawcall number {drawcall}: {error}"
        ))
    })?;
    Ok(Some((drawcall, remainder.trim_start())))
}

fn apply_api_call(
    draw_call: &mut FrameAnalysisDrawCall,
    api_call: &str,
) -> AppResult<Option<PendingBindingKind>> {
    let api_name = api_call
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or(api_call)
        .trim();

    match api_name {
        "IASetVertexBuffers" => Ok(Some(PendingBindingKind::VertexBuffer)),
        "IASetIndexBuffer" => {
            draw_call.ib_format = parse_named_string_argument(api_call, "Format")
                .map(|raw| normalize_index_format(&raw));
            // Live WWMI dump format: `IASetIndexBuffer(...) hash=<hex>` on a single
            // line. Synthetic fixture format: hash on indented continuation line.
            if let Some(hash) = extract_inline_trailing_hash(api_call)? {
                let resource =
                    parse_named_string_argument(api_call, "pIndexBuffer").unwrap_or_default();
                draw_call.ib_binding = Some(FrameAnalysisBinding {
                    slot: DEFAULT_BINDING_SLOT.to_string(),
                    view_address: None,
                    resource_address: resource,
                    hash,
                });
                return Ok(Some(PendingBindingKind::Ignore));
            }
            Ok(Some(PendingBindingKind::IndexBuffer))
        }
        "VSSetShader" => {
            if let Some(hash) = extract_inline_trailing_hash(api_call)? {
                let resource =
                    parse_named_string_argument(api_call, "pVertexShader").unwrap_or_default();
                draw_call.vs_binding = Some(FrameAnalysisBinding {
                    slot: DEFAULT_BINDING_SLOT.to_string(),
                    view_address: None,
                    resource_address: resource,
                    hash,
                });
                return Ok(Some(PendingBindingKind::Ignore));
            }
            Ok(Some(PendingBindingKind::VertexShader))
        }
        "PSSetShader" => {
            if let Some(hash) = extract_inline_trailing_hash(api_call)? {
                let resource =
                    parse_named_string_argument(api_call, "pPixelShader").unwrap_or_default();
                draw_call.ps_binding = Some(FrameAnalysisBinding {
                    slot: DEFAULT_BINDING_SLOT.to_string(),
                    view_address: None,
                    resource_address: resource,
                    hash,
                });
                return Ok(Some(PendingBindingKind::Ignore));
            }
            Ok(Some(PendingBindingKind::PixelShader))
        }
        "CSSetShader" => {
            if let Some(hash) = extract_inline_trailing_hash(api_call)? {
                let resource =
                    parse_named_string_argument(api_call, "pComputeShader").unwrap_or_default();
                draw_call.cs_binding = Some(FrameAnalysisBinding {
                    slot: DEFAULT_BINDING_SLOT.to_string(),
                    view_address: None,
                    resource_address: resource,
                    hash,
                });
                return Ok(Some(PendingBindingKind::Ignore));
            }
            // No inline hash typically means the CS was unbound
            // (`pComputeShader:0x0000000000000000`). Ignore without error.
            Ok(Some(PendingBindingKind::Ignore))
        }
        "PSSetShaderResources" => Ok(Some(PendingBindingKind::PixelShaderTextures)),
        "SOSetTargets" => Ok(Some(PendingBindingKind::Ignore)),
        "Dispatch" => {
            draw_call.draw = Some(FrameAnalysisDraw::Compute {
                thread_group_x: parse_required_u32_argument(api_call, "ThreadGroupCountX")?,
                thread_group_y: parse_required_u32_argument(api_call, "ThreadGroupCountY")?,
                thread_group_z: parse_required_u32_argument(api_call, "ThreadGroupCountZ")?,
            });
            Ok(None)
        }
        "DispatchIndirect" => {
            // Thread group counts are not recoverable from the API line for
            // an indirect dispatch; store zeros as a sentinel so downstream
            // can still recognise the draw call as compute-kind.
            draw_call.draw = Some(FrameAnalysisDraw::Compute {
                thread_group_x: 0,
                thread_group_y: 0,
                thread_group_z: 0,
            });
            Ok(None)
        }
        "DrawIndexed" => {
            draw_call.draw = Some(FrameAnalysisDraw::Indexed {
                index_count: parse_required_u32_argument(api_call, "IndexCount")?,
                start_index: parse_required_u32_argument(api_call, "StartIndexLocation")?,
                base_vertex: parse_required_i32_argument(api_call, "BaseVertexLocation")?,
            });
            Ok(None)
        }
        "Draw" => {
            draw_call.draw = Some(FrameAnalysisDraw::NonIndexed {
                vertex_count: parse_required_u32_argument(api_call, "VertexCount")?,
                start_vertex: parse_required_u32_argument(api_call, "StartVertexLocation")?,
            });
            Ok(None)
        }
        _ => Ok(Some(PendingBindingKind::Ignore)),
    }
}

fn parse_binding_line(line: &str, _kind: PendingBindingKind) -> AppResult<FrameAnalysisBinding> {
    let trimmed = line.trim();
    let (slot, remainder) = if let Some((slot, remainder)) = trimmed.split_once(':') {
        if slot
            .chars()
            .all(|character| character.is_ascii_digit() || character == 'D')
        {
            (slot.to_string(), remainder.trim_start())
        } else {
            (DEFAULT_BINDING_SLOT.to_string(), trimmed)
        }
    } else {
        (DEFAULT_BINDING_SLOT.to_string(), trimmed)
    };

    let mut view_address = None;
    let mut resource_address = None;
    let mut hash = None;
    for part in remainder.split_whitespace() {
        if let Some(value) = part.strip_prefix("view=") {
            view_address = Some(value.to_string());
            continue;
        }
        if let Some(value) = part.strip_prefix("resource=") {
            resource_address = Some(value.to_string());
            continue;
        }
        if let Some(value) = part.strip_prefix("hash=") {
            if !value
                .chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
            {
                return Err(AppError::InvalidInput(format!(
                    "frame analysis hash must be lowercase hex, got {value}"
                )));
            }
            hash = Some(value.to_string());
        }
    }

    Ok(FrameAnalysisBinding {
        slot,
        view_address,
        resource_address: resource_address.ok_or_else(|| {
            AppError::InvalidInput(format!("binding line missing resource=... field: {line}"))
        })?,
        hash: hash.ok_or_else(|| {
            AppError::InvalidInput(format!("binding line missing hash=... field: {line}"))
        })?,
    })
}

fn parse_required_u32_argument(api_call: &str, key: &str) -> AppResult<u32> {
    let value = parse_named_string_argument(api_call, key).ok_or_else(|| {
        AppError::InvalidInput(format!("frame analysis call missing {key}: {api_call}"))
    })?;
    value.parse::<u32>().map_err(|error| {
        AppError::InvalidInput(format!(
            "frame analysis call has invalid {key} value {value}: {error}"
        ))
    })
}

fn parse_required_i32_argument(api_call: &str, key: &str) -> AppResult<i32> {
    let value = parse_named_string_argument(api_call, key).ok_or_else(|| {
        AppError::InvalidInput(format!("frame analysis call missing {key}: {api_call}"))
    })?;
    value.parse::<i32>().map_err(|error| {
        AppError::InvalidInput(format!(
            "frame analysis call has invalid {key} value {value}: {error}"
        ))
    })
}

/// Parse a trailing `hash=<hex>` segment that follows the closing `)` of an
/// API call line. Live WWMI/3DMigoto dumps emit the resource hash inline on
/// the same line as `IASetIndexBuffer`, `VSSetShader`, and `PSSetShader`,
/// rather than on an indented continuation line. Returns `None` when no such
/// trailing token is present (the parser then falls back to the indented
/// `resource=... hash=...` continuation form used by synthetic fixtures).
fn extract_inline_trailing_hash(api_call: &str) -> AppResult<Option<String>> {
    let Some(close_paren) = api_call.rfind(')') else {
        return Ok(None);
    };
    let trailing = api_call[close_paren + 1..].trim();
    let Some(value) = trailing.strip_prefix("hash=") else {
        return Ok(None);
    };
    let hash: String = value
        .chars()
        .take_while(|character| !character.is_ascii_whitespace())
        .collect();
    if hash.is_empty() {
        return Ok(None);
    }
    if !hash
        .chars()
        .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        return Err(AppError::InvalidInput(format!(
            "frame analysis hash must be lowercase hex, got {hash}"
        )));
    }
    Ok(Some(hash))
}

/// Normalize an `IASetIndexBuffer` `Format:` argument to a canonical string so
/// that fixture-style symbolic names (`R32_UINT`) and live-dump numeric DXGI
/// values (`42`, `57`) compare equal downstream.
fn normalize_index_format(raw: &str) -> String {
    let trimmed = raw.trim();
    match trimmed {
        "42" | "DXGI_FORMAT_R32_UINT" | "R32_UINT" => "R32_UINT".to_string(),
        "57" | "DXGI_FORMAT_R16_UINT" | "R16_UINT" => "R16_UINT".to_string(),
        other => other.to_string(),
    }
}

fn parse_named_string_argument(api_call: &str, key: &str) -> Option<String> {
    let start = api_call.find('(')?;
    let end = api_call.rfind(')')?;
    let inner = &api_call[start + 1..end];
    inner.split(',').find_map(|part| {
        let (name, value) = part.trim().split_once(':')?;
        if name.trim() == key {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn build_shader_asset(
    shader_prefix: &str,
    shader_kind: &str,
    hash: String,
    draw_call_count: usize,
    dump_root: Option<String>,
) -> ExtractedAssetRecord {
    ExtractedAssetRecord {
        asset: AssetRecord {
            id: format!("{shader_prefix}_{hash}"),
            path: format!("runtime/{shader_prefix}/{hash}"),
            kind: Some(shader_kind.to_string()),
            metadata: AssetMetadata {
                logical_name: Some(format!("{shader_prefix}_{hash}")),
                tags: vec![format!("draw_calls={draw_call_count}")],
                ..AssetMetadata::default()
            },
        },
        hash_fields: AssetHashFields {
            asset_hash: None,
            shader_hash: Some(hash),
            signature: None,
            identity_tuple: None,
        },
        source: frame_analysis_source_context(dump_root),
    }
}

fn shader_identity_tuple(shader_hash: Option<&str>) -> Option<String> {
    shader_hash.map(|value| format!("fa|vb|shader:{value}"))
}

fn compute_shader_identity_tuple(aggregate: &CsAggregate) -> Option<String> {
    // When a compute shader is dispatched with exactly one observed thread
    // group shape, that shape is a stable-ish fingerprint across a hash
    // drift caused by a semantics-preserving recompile. When we see
    // multiple shapes (or none, for `DispatchIndirect`), fall back to None
    // and let path-keyed comparison apply.
    if aggregate.thread_group_dims.len() == 1 {
        let (x, y, z) = aggregate.thread_group_dims.iter().next().copied().unwrap();
        if (x, y, z) == (0, 0, 0) {
            None
        } else {
            Some(format!("fa|cs|tg:{x}x{y}x{z}"))
        }
    } else {
        None
    }
}

fn ps_texture_identity_tuple(ps_hash: Option<&str>, slot: Option<&str>) -> Option<String> {
    match (ps_hash, slot) {
        (Some(ps), Some(slot)) => Some(format!("fa|tex|ps:{ps}|slot:{slot}")),
        _ => None,
    }
}

fn frame_analysis_source_context(source_root: Option<String>) -> AssetSourceContext {
    AssetSourceContext {
        extraction_tool: Some(FRAME_ANALYSIS_TOOL.to_string()),
        source_root,
        source_path: Some("log.txt".to_string()),
        container_path: None,
        source_kind: Some(RUNTIME_DRAW_CALL_KIND.to_string()),
    }
}

fn unique_ib_hash_count(dump: &FrameAnalysisDump) -> usize {
    dump.draw_calls
        .iter()
        .filter_map(|draw_call| {
            draw_call
                .ib_binding
                .as_ref()
                .map(|binding| binding.hash.clone())
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn unique_vb_hash_count(dump: &FrameAnalysisDump) -> usize {
    dump.draw_calls
        .iter()
        .flat_map(|draw_call| {
            draw_call
                .vb_bindings
                .iter()
                .map(|binding| binding.hash.clone())
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn normalize_optional_path(path: &PathBuf) -> Option<String> {
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path.to_string_lossy().replace('\\', "/"))
    }
}

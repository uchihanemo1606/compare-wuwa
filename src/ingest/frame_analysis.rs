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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingBindingKind {
    VertexBuffer,
    IndexBuffer,
    VertexShader,
    PixelShader,
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

impl FrameAnalysisDrawCall {
    fn is_meaningful(&self) -> bool {
        !self.vb_bindings.is_empty()
            || self.ib_binding.is_some()
            || self.vs_binding.is_some()
            || self.ps_binding.is_some()
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
    if !options_header.starts_with("analyse_options=") {
        return Err(AppError::InvalidInput(
            "frame analysis log must start with analyse_options=...".to_string(),
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
    }

    let mut assets = Vec::<ExtractedAssetRecord>::new();

    for (hash, aggregate) in ib_assets {
        assets.push(ExtractedAssetRecord {
            asset: AssetRecord {
                id: format!("ib_{hash}"),
                path: format!("runtime/ib/{hash}"),
                kind: Some("index_buffer".to_string()),
                metadata: AssetMetadata {
                    logical_name: Some(format!("ib_{hash}")),
                    index_count: aggregate.max_index_count,
                    index_format: aggregate.index_format,
                    tags: vec![format!("draw_calls={}", aggregate.draw_calls.len())],
                    ..AssetMetadata::default()
                },
            },
            hash_fields: AssetHashFields {
                asset_hash: Some(hash),
                shader_hash: None,
                signature: None,
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
            draw_call.ib_format = parse_named_string_argument(api_call, "Format");
            Ok(Some(PendingBindingKind::IndexBuffer))
        }
        "VSSetShader" => Ok(Some(PendingBindingKind::VertexShader)),
        "PSSetShader" => Ok(Some(PendingBindingKind::PixelShader)),
        "SOSetTargets" => Ok(Some(PendingBindingKind::Ignore)),
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
        },
        source: frame_analysis_source_context(dump_root),
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

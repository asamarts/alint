//! Core fix edits to LSP `WorkspaceEdit`s.

use std::collections::HashMap;
use std::path::Path;

use lsp_types::{
    CreateFile, DeleteFile, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp,
    TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

use alint_core::{CollectedEdit, FixEdit};

use crate::diagnostics::{byte_offset_to_position, whole_document};

/// Which `WorkspaceEdit.documentChanges` resource operations the client
/// advertised in `capabilities.workspace.workspaceEdit`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ResourceOpSupport {
    pub(crate) create: bool,
    pub(crate) delete: bool,
    pub(crate) rename: bool,
}

impl ResourceOpSupport {
    #[cfg(test)]
    pub(crate) const fn all() -> Self {
        Self {
            create: true,
            delete: true,
            rename: true,
        }
    }
}

/// Map a located fixer's collected byte-range edits (Phase 1 `replace`) to an LSP
/// [`WorkspaceEdit`] for one file: each [`FixEdit::ReplaceRange`] becomes a
/// [`TextEdit`] whose range is the byte offsets converted to UTF-16 positions
/// against `text` (the current buffer, which is what the offsets index).
/// `collect_edits` yields DISJOINT, left-to-right matches, so the `TextEdit`s never
/// overlap -- exactly what the LSP requires of a single edit set. Returns `None`
/// when there are no range edits, a range is out-of-bounds or mid-scalar, or a
/// replacement isn't UTF-8, so the caller offers no action rather than a partial
/// or corrupt one.
pub(crate) fn located_edits_to_workspace_edit(
    edits: &[CollectedEdit],
    text: &str,
    doc_uri: &Url,
) -> Option<WorkspaceEdit> {
    let mut text_edits = Vec::new();
    for ce in edits {
        // Every located fixer today (ReplaceFixer, the only one) emits ReplaceRange.
        // A future located fixer that emitted a whole-file edit here would have it
        // silently dropped -- pin the invariant so that regresses LOUDLY in debug,
        // and skip (never mis-apply) in release.
        let FixEdit::ReplaceRange { range, content, .. } = &ce.edit else {
            debug_assert!(false, "located edit is not a ReplaceRange: {:?}", ce.edit);
            continue;
        };
        if range.start > range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return None;
        }
        let new_text = String::from_utf8(content.clone()).ok()?;
        let start = byte_offset_to_position(text, range.start);
        let end = byte_offset_to_position(text, range.end);
        text_edits.push(TextEdit {
            range: Range::new(start, end),
            new_text,
        });
    }
    if text_edits.is_empty() {
        return None;
    }
    let mut changes = HashMap::new();
    changes.insert(doc_uri.clone(), text_edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn edit_uri(path: &Path, root: &Path, active_doc: Option<(&Path, &Url)>) -> Option<Url> {
    if let Some((rel, uri)) = active_doc
        && path == rel
    {
        return Some(uri.clone());
    }
    Url::from_file_path(root.join(path)).ok()
}

/// Map a core [`FixEdit`] to an LSP [`WorkspaceEdit`]. Content edits use
/// the widely-supported `changes` map; create/delete/rename use resource
/// operations only when the client advertised support in `resource_ops`.
/// Returns `None` when content isn't UTF-8, a path can't become a URI, or
/// the client did not advertise the needed resource operation.
pub(crate) fn fix_edit_to_workspace_edit(
    edit: &FixEdit,
    root: &Path,
    active_doc: Option<(&Path, &Url)>,
    resource_ops: ResourceOpSupport,
) -> Option<WorkspaceEdit> {
    match edit {
        FixEdit::SetContent { path, content } => {
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = edit_uri(path, root, active_doc)?;
            let mut changes = HashMap::new();
            changes.insert(
                uri,
                vec![TextEdit {
                    range: whole_document(),
                    new_text,
                }],
            );
            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        }
        FixEdit::CreateFile { path, content } => {
            if !resource_ops.create {
                return None;
            }
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = edit_uri(path, root, active_doc)?;
            let ops = vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: uri.clone(),
                    options: None,
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        new_text,
                    })],
                }),
            ];
            Some(operations(ops))
        }
        FixEdit::DeleteFile { path } => {
            if !resource_ops.delete {
                return None;
            }
            let uri = edit_uri(path, root, active_doc)?;
            Some(operations(vec![DocumentChangeOperation::Op(
                ResourceOp::Delete(DeleteFile { uri, options: None }),
            )]))
        }
        FixEdit::RenameFile { from, to } => {
            if !resource_ops.rename {
                return None;
            }
            let old_uri = edit_uri(from, root, active_doc)?;
            let new_uri = edit_uri(to, root, active_doc)?;
            Some(operations(vec![DocumentChangeOperation::Op(
                ResourceOp::Rename(RenameFile {
                    old_uri,
                    new_uri,
                    options: None,
                    annotation_id: None,
                }),
            )]))
        }
        // A `ReplaceRange` reaches the LSP through `collect_edits` (a located
        // fixer's real path), mapped to UTF-16 `TextEdit`s by
        // `located_edits_to_workspace_edit` -- NOT through `fix_edit`, which no
        // located fixer implements (it returns `None`). So a `ReplaceRange` here
        // is unreachable, and there is no buffer context to map its byte offsets
        // anyway. A `chmod` (`SetMode`) has no LSP `WorkspaceEdit` representation at
        // all. Both map to `None`.
        FixEdit::ReplaceRange { .. } | FixEdit::SetMode { .. } => None,
    }
}

fn operations(ops: Vec<DocumentChangeOperation>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(ops)),
        change_annotations: None,
    }
}

//! Core fix edits to LSP `WorkspaceEdit`s.

use std::collections::HashMap;
use std::path::Path;

use tower_lsp::lsp_types::{
    CreateFile, DeleteFile, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp,
    TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

use alint_core::{CollectedEdit, FixEdit};

use crate::diagnostics::{byte_offset_to_position, whole_document};

/// Map a located fixer's collected byte-range edits (Phase 1 `replace`) to an LSP
/// [`WorkspaceEdit`] for one file: each [`FixEdit::ReplaceRange`] becomes a
/// [`TextEdit`] whose range is the byte offsets converted to UTF-16 positions
/// against `text` (the current buffer, which is what the offsets index).
/// `collect_edits` yields DISJOINT, left-to-right matches, so the `TextEdit`s never
/// overlap -- exactly what the LSP requires of a single edit set. Returns `None`
/// when there are no range edits, a replacement isn't UTF-8, or the path can't
/// become a URI, so the caller offers no action rather than a partial one.
pub(crate) fn located_edits_to_workspace_edit(
    edits: &[CollectedEdit],
    text: &str,
    rel: &Path,
    root: &Path,
) -> Option<WorkspaceEdit> {
    let uri = Url::from_file_path(root.join(rel)).ok()?;
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
    changes.insert(uri, text_edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

/// Map a core [`FixEdit`] to an LSP [`WorkspaceEdit`]. Content edits use
/// the widely-supported `changes` map; create/delete/rename use resource
/// operations (the client must advertise `resourceOperations` support).
/// Returns `None` when content isn't UTF-8 or a path can't become a URI.
pub(crate) fn fix_edit_to_workspace_edit(edit: &FixEdit, root: &Path) -> Option<WorkspaceEdit> {
    match edit {
        FixEdit::SetContent { path, content } => {
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = Url::from_file_path(root.join(path)).ok()?;
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
            let new_text = String::from_utf8(content.clone()).ok()?;
            let uri = Url::from_file_path(root.join(path)).ok()?;
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
            let uri = Url::from_file_path(root.join(path)).ok()?;
            Some(operations(vec![DocumentChangeOperation::Op(
                ResourceOp::Delete(DeleteFile { uri, options: None }),
            )]))
        }
        FixEdit::RenameFile { from, to } => {
            let old_uri = Url::from_file_path(root.join(from)).ok()?;
            let new_uri = Url::from_file_path(root.join(to)).ok()?;
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

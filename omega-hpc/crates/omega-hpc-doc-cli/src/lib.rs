mod cli;
mod exit_codes;
mod output;

pub use cli::run;
pub use output::CliOutput;

#[cfg(test)]
mod tests {
    use std::fs;

    use omega_hpc_document::{
        DocType, DocumentMutationMode, DocumentOp, OmegaDocument, StructuredDocRelationRecord,
        StructuredDocumentRecord, StructuredDocumentRelation, StructuredDocumentRender,
        StructuredDocumentSection,
    };
    use tempfile::tempdir;

    use crate::run;

    #[test]
    fn get_returns_record_by_id() {
        let root = tempdir().unwrap();
        let documents = seed_record(root.path());
        documents
            .manage_document(DocumentOp::RenderProjection {
                mode: DocumentMutationMode::Apply,
                doc_ids: Vec::new(),
            })
            .unwrap();

        let output = run(vec![
            "get".to_string(),
            "spec:test-doc".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
            "--json".to_string(),
        ]);

        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("\"kind\": \"record\""));
        assert!(output.stdout.contains("spec:test-doc"));
        assert!(output.stdout.contains("\"content_revision\":"));
        assert!(output.stdout.contains("\"projection_version\":"));
    }

    #[test]
    fn doctor_reports_version_information_after_render() {
        let root = tempdir().unwrap();
        let documents = seed_record(root.path());
        documents
            .manage_document(DocumentOp::RenderProjection {
                mode: DocumentMutationMode::Apply,
                doc_ids: Vec::new(),
            })
            .unwrap();

        let output = run(vec![
            "doctor".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
            "--json".to_string(),
        ]);

        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("\"version\":"));
        assert!(output.stdout.contains("\"content_revision\":"));
        assert!(output.stdout.contains("\"projection_version\":"));
        assert!(output.stdout.contains("\"validation\":"));
    }

    #[test]
    fn record_remove_deletes_generated_projection() {
        let root = tempdir().unwrap();
        let documents = seed_record(root.path());
        documents
            .manage_document(DocumentOp::RenderProjection {
                mode: DocumentMutationMode::Apply,
                doc_ids: Vec::new(),
            })
            .unwrap();
        assert!(root.path().join("docs/specs/test-doc.md").exists());

        let output = run(vec![
            "remove".to_string(),
            "spec:test-doc".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
        ]);

        assert_eq!(output.exit_code, 0);
        assert!(!root.path().join("docs/specs/test-doc.md").exists());
    }

    #[test]
    fn cutover_extracts_and_renders_docs() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("docs/specs")).unwrap();
        fs::write(
            root.path().join("docs/specs/from-markdown.md"),
            "---\nstatus: draft\nowner: omega-team\n---\n\n# From Markdown\n\n## Overview\n\nBody\n",
        )
        .unwrap();

        let output = run(vec![
            "cutover".to_string(),
            "docs/specs/from-markdown.md".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
        ]);

        assert_eq!(output.exit_code, 0);
        assert!(root.path().join("docs-data/records/specs.jsonl").exists());
    }

    #[test]
    fn archive_moves_doc_into_archive_path() {
        let root = tempdir().unwrap();
        let documents = seed_record(root.path());
        documents
            .manage_document(DocumentOp::RenderProjection {
                mode: DocumentMutationMode::Apply,
                doc_ids: Vec::new(),
            })
            .unwrap();

        let output = run(vec![
            "archive".to_string(),
            "spec:test-doc".to_string(),
            "--reason".to_string(),
            "history".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
        ]);

        assert_eq!(output.exit_code, 0);
        assert!(!root.path().join("docs/specs/test-doc.md").exists());

        let render = run(vec![
            "render".to_string(),
            "spec:test-doc".to_string(),
            "--root".to_string(),
            root.path().display().to_string(),
        ]);
        assert_eq!(render.exit_code, 0);
        assert!(root.path().join("docs/archive/test-doc.md").exists());
    }

    fn seed_record(root: &std::path::Path) -> OmegaDocument {
        fs::create_dir_all(root.join("docs/specs")).unwrap();
        let documents = OmegaDocument::new(root.to_path_buf());
        documents
            .manage_document(DocumentOp::UpsertRecord {
                mode: DocumentMutationMode::Apply,
                record: StructuredDocumentRecord {
                    doc_id: "spec:test-doc".to_string(),
                    doc_type: DocType::Spec,
                    slug: "test-doc".to_string(),
                    title: "Test Doc".to_string(),
                    status: Some("draft".to_string()),
                    owner: Some("omega-team".to_string()),
                    created: Some("2026-04-14".to_string()),
                    updated: Some("2026-04-14".to_string()),
                    version: None,
                    source_path: "docs/specs/test-doc.md".to_string(),
                    frontmatter: Default::default(),
                    sections: vec![StructuredDocumentSection {
                        section_id: "overview".to_string(),
                        heading: "Overview".to_string(),
                        body_markdown: "Body".to_string(),
                    }],
                    relations: vec![StructuredDocumentRelation {
                        kind: "references".to_string(),
                        target: "docs/specs/test-doc.md".to_string(),
                    }],
                    render: StructuredDocumentRender {
                        template: "spec-v1".to_string(),
                        presentation_path: "docs/specs/test-doc.md".to_string(),
                    },
                },
            })
            .unwrap();
        documents
            .manage_document(DocumentOp::UpsertRelation {
                mode: DocumentMutationMode::Apply,
                relation: StructuredDocRelationRecord {
                    relation_id: "rel:test".to_string(),
                    source: "spec:test-doc".to_string(),
                    kind: "references".to_string(),
                    target: "spec:test-doc".to_string(),
                    metadata: Default::default(),
                },
            })
            .unwrap();
        documents
    }
}

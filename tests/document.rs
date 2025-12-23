use anyhow::Result;
use writ::document::{Document, ToMarkdown};

#[test]
fn test_lists() -> Result<()> {
    let cases = [
        r#"- first item
- second item
- third item"#,
        r#"1. first item
2. second item
3. third item"#,
        r#"- first item
  - nested item
  - second item"#,
    ];
    for case in cases {
        let d = Document::from_markdown(case)?;
        let md = d.to_markdown();
        assert_eq!(case, md);
    }
    Ok(())
}

#[test]
fn test_inlines() -> Result<()> {
    let cases = [
        "# Header 1",
        "## Header 2",
        "### Header 3",
        "#### Header 4",
        "##### Header 5",
        "###### Header 6",
        "# Header *with italic*",
        "# Header **with bold**",
        "# Header ***with both***",
        "# Header *italic* **bold**",
        "# Header *italic **bold** and*",
        "# Header **bold *italic***",
        "## Header `with code` foo",
        "**Bold at the start** of a line.",
        "*Italic at the start* of a line.",
        "`Code at the start` of a line.",
        "This paragragh has all examples *with italic* and **with bold** and ***with both*** and *italic* **bold** and *italic **bold** and* and **bold *italic*** and `with code` foo",
    ];
    for case in cases {
        let d = Document::from_markdown(case)?;
        let md = d.to_markdown();
        assert_eq!(case, md);
    }
    Ok(())
}

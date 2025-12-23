use anyhow::Result;
use writ::document::{Document, ToMarkdown};

#[test]
fn test_lists() -> Result<()> {
    let cases = [
        r#"- first item
- second item
- third item
"#,
        r#"1. first item
2. second item
3. third item
"#,
        r#"- first item
  - nested item
  - second item
"#,
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
        "# Header 1\n",
        "## Header 2\n",
        "### Header 3\n",
        "#### Header 4\n",
        "##### Header 5\n",
        "###### Header 6\n",
        "# Header *with italic*\n",
        "# Header **with bold**\n",
        "# Header ***with both***\n",
        "# Header *italic* **bold**\n",
        "# Header *italic **bold** and*\n",
        "# Header **bold *italic***\n",
        "## Header `with code` foo\n",
        "**Bold at the start** of a line.\n",
        "*Italic at the start* of a line.\n",
        "`Code at the start` of a line.\n",
        "This paragragh has all examples *with italic* and **with bold** and ***with both*** and *italic* **bold** and *italic **bold** and* and **bold *italic*** and `with code` foo\n",
    ];
    for case in cases {
        let d = Document::from_markdown(case)?;
        let md = d.to_markdown();
        assert_eq!(case, md);
    }
    Ok(())
}

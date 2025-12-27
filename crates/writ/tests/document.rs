use writ::document::Document;

#[test]
#[should_panic(expected = "inline images are not supported")]
fn test_inline_image_text_before_panics() {
    Document::from_markdown("hello ![alt](http://example.com/image.png)\n");
}

#[test]
#[should_panic(expected = "inline images are not supported")]
fn test_inline_image_text_after_panics() {
    Document::from_markdown("![alt](http://example.com/image.png) world\n");
}

#[test]
fn test_nested_numbered_list_indentation() {
    // Original uses 3-space indentation
    let input = "1. Numbered item 1
   1. Nested numbered 1.1
   2. Nested numbered 1.2
2. Numbered item 2
";
    let doc1 = Document::from_markdown(input);
    let output1 = doc1.to_markdown();

    let doc2 = Document::from_markdown(&output1);
    let output2 = doc2.to_markdown();

    // After one roundtrip, it should be stable
    assert_eq!(output1, output2, "Output should be stable after roundtrip");
}

#[test]
fn test_lists() {
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
        let d = Document::from_markdown(case);
        let md = d.to_markdown();
        assert_eq!(case, md);
    }
}

#[test]
fn test_inlines() {
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
        let d = Document::from_markdown(case);
        let md = d.to_markdown();
        assert_eq!(case, md);
    }
}

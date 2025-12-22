use anyhow::Result;
use itertools::Itertools;
use pulldown_cmark::{Event, Parser as MarkdownParser, Tag, TagEnd};
use std::{fs, ops::Range};

#[derive(Debug, Clone, PartialEq)]
pub enum SpanStyle {
    Bold,
    Italic,
    Code,
    Link { url: String },
    Strikethrough,
}

impl SpanStyle {
    pub fn start_marker(&self) -> String {
        match self {
            SpanStyle::Bold => "**",
            SpanStyle::Italic => "*",
            SpanStyle::Code => "`",
            SpanStyle::Link { .. } => "[",
            SpanStyle::Strikethrough => "~~",
        }
        .to_string()
    }

    pub fn end_marker(&self) -> String {
        match self {
            SpanStyle::Bold => "**".to_string(),
            SpanStyle::Italic => "*".to_string(),
            SpanStyle::Code => "`".to_string(),
            SpanStyle::Link { url } => format!("]({})", url),
            SpanStyle::Strikethrough => "~~".to_string(),
        }
    }
}

#[derive(Debug)]
enum SpanEvent {
    Open(SpanStyle),
    Close(SpanStyle),
}

#[derive(Debug, Clone)]
pub struct FormatSpan {
    pub range: Range<usize>,
    pub style: SpanStyle,
}

#[derive(Debug, Clone)]
pub struct RichText {
    pub text: String,
    pub spans: Vec<FormatSpan>,
}

impl RichText {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            spans: Vec::new(),
        }
    }

    pub fn to_markdown(&self) -> String {
        if self.spans.is_empty() {
            return self.text.clone();
        }

        let mut events: Vec<(usize, SpanEvent)> = Vec::new();

        for span in &self.spans {
            events.push((span.range.start, SpanEvent::Open(span.style.clone())));
            events.push((span.range.end, SpanEvent::Close(span.style.clone())));
        }

        events.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| match (&a.1, &b.1) {
                (SpanEvent::Close(_), SpanEvent::Open(_)) => std::cmp::Ordering::Less,
                (SpanEvent::Open(_), SpanEvent::Close(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
        });

        let mut text_iter = self.text.chars().enumerate().peekable();
        let mut text = String::new();
        for (index, event) in events {
            let segment = text_iter
                .peeking_take_while(|(i, _)| *i < index)
                .map(|(_, c)| c);
            text.extend(segment);
            match &event {
                SpanEvent::Open(style) => text.push_str(&style.start_marker()),
                SpanEvent::Close(style) => {
                    text.push_str(&style.end_marker());
                }
            }
        }

        text.extend(text_iter.map(|(_, c)| c));
        text
    }
}

impl Default for RichText {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Paragraph,
    Heading { level: u8, id: Option<String> },
    CodeBlock { language: Option<String> },
    BulletList,
    NumberedList,
    Quote,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub content: RichText,
}

impl Block {
    pub fn new_paragraph() -> Self {
        Self {
            kind: BlockKind::Paragraph,
            content: RichText {
                text: String::new(),
                spans: Vec::new(),
            },
        }
    }

    pub fn to_markdown(&self) -> String {
        let md = self.content.to_markdown();

        match &self.kind {
            // BlockKind::Paragraph => content_md,
            BlockKind::Heading { level, .. } => {
                format!("{} {}", "#".repeat(*level as usize), md)
            }
            BlockKind::Paragraph => md,
            _ => todo!("block kind"),
        }
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::new_paragraph()
    }
}

pub struct Parser {
    blocks: Vec<Block>,
    block: Block,
    stack: Vec<usize>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            block: Block::default(),
            stack: Vec::new(),
        }
    }

    fn open_span(&mut self, style: SpanStyle) {
        let start = self.block.content.text.len();
        self.block.content.spans.push(FormatSpan {
            range: start..start,
            style,
        });
        self.stack.push(self.block.content.spans.len() - 1);
    }

    fn close_span(&mut self) {
        if let Some(index) = self.stack.pop() {
            let span = &mut self.block.content.spans[index];
            span.range.end = self.block.content.text.len();
        }
    }

    fn push_block(&mut self) {
        let block = self.block.clone();
        self.blocks.push(block);
        self.block = Block::default();
        self.stack.clear();
    }

    pub fn parse(mut self, parser: MarkdownParser) -> Vec<Block> {
        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => self.block.kind = BlockKind::Paragraph,
                    Tag::Heading { level, id, .. } => {
                        self.block.kind = BlockKind::Heading {
                            level: level as u8,
                            id: id.map(|id| id.to_string()),
                        };
                    }
                    Tag::BlockQuote(_block_quote_kind) => todo!("block quote"),
                    Tag::CodeBlock(_code_block_kind) => todo!("code block"),
                    Tag::HtmlBlock => todo!("html block"),
                    Tag::List(_) => todo!("list"),
                    Tag::Item => todo!("item"),
                    Tag::FootnoteDefinition(_cow_str) => todo!("footnote definition"),
                    Tag::DefinitionList => todo!("definition list"),
                    Tag::DefinitionListTitle => todo!("definition list title"),
                    Tag::DefinitionListDefinition => todo!("definition list definition"),
                    Tag::Table(_alignments) => todo!("table"),
                    Tag::TableHead => todo!("table head"),
                    Tag::TableRow => todo!("table row"),
                    Tag::TableCell => todo!("table cell"),
                    Tag::Emphasis => self.open_span(SpanStyle::Italic),
                    Tag::Strong => self.open_span(SpanStyle::Bold),
                    Tag::Strikethrough => self.open_span(SpanStyle::Strikethrough),
                    Tag::Superscript => todo!("superscript"),
                    Tag::Subscript => todo!("subscript"),
                    Tag::Link { .. } => todo!("link"),
                    Tag::Image { .. } => todo!("image"),
                    Tag::MetadataBlock(_metadata_block_kind) => todo!("metadata block"),
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Paragraph | TagEnd::Heading(_) => {
                        self.push_block();
                    }
                    TagEnd::BlockQuote(_block_quote_kind) => todo!("block quote"),
                    TagEnd::CodeBlock => todo!("code block"),
                    TagEnd::HtmlBlock => todo!("html block"),
                    TagEnd::List(_) => todo!("list"),
                    TagEnd::Item => todo!("item"),
                    TagEnd::FootnoteDefinition => todo!("footnote definition"),
                    TagEnd::DefinitionList => todo!("definition list"),
                    TagEnd::DefinitionListTitle => todo!("definition list title"),
                    TagEnd::DefinitionListDefinition => todo!("definition list definition"),
                    TagEnd::Table => todo!("table"),
                    TagEnd::TableHead => todo!("table head"),
                    TagEnd::TableRow => todo!("table row"),
                    TagEnd::TableCell => todo!("table cell"),
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.close_span(),
                    TagEnd::Superscript => todo!("superscript"),
                    TagEnd::Subscript => todo!("subscript"),
                    TagEnd::Link => todo!("link"),
                    TagEnd::Image => todo!("image"),
                    TagEnd::MetadataBlock(_metadata_block_kind) => todo!("metadata block"),
                },
                Event::Text(cow_str) => {
                    println!("Text: {}", cow_str);
                    self.block.content.text += &cow_str
                }
                Event::Code(cow_str) => {
                    self.open_span(SpanStyle::Code);
                    self.block.content.text += &cow_str;
                    self.close_span();
                }
                Event::InlineMath(_cow_str) => todo!("inline math"),
                Event::DisplayMath(_cow_str) => todo!("display math"),
                Event::Html(_cow_str) => todo!("html"),
                Event::InlineHtml(_cow_str) => todo!("inline html"),
                Event::FootnoteReference(_cow_str) => todo!("footnote reference"),
                Event::SoftBreak => todo!("soft break"),
                Event::HardBreak => todo!("hard break"),
                Event::Rule => todo!("rule"),
                Event::TaskListMarker(_) => todo!("task list marker"),
            }
        }

        self.blocks
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn new() -> Self {
        Document {
            blocks: vec![Block::default()],
        }
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_markdown(&content)
    }

    pub fn from_markdown(markdown: &str) -> Result<Self> {
        let parser = MarkdownParser::new(markdown);
        let blocks = Parser::new().parse(parser);
        Ok(Document { blocks })
    }

    pub fn to_markdown(&self) -> String {
        self.blocks
            .iter()
            .map(|block| block.to_markdown())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
        let markdown = self.to_markdown();
        fs::write(path, markdown)?;
        Ok(())
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

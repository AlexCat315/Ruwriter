use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

use super::{Chapter, MAX_BOOK_BYTES, invalid};

fn member(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> io::Result<String> {
    let file = zip
        .by_name(name)
        .map_err(|error| invalid(format!("无法读取 EPUB 内的 {name}。{error}")))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("EPUB 解包后的正文超过 128 MB。"));
    }
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(&bytes) {
        return encoding
            .decode_without_bom_handling_and_without_replacement(&bytes[bom..])
            .map(|text| text.into_owned())
            .ok_or_else(|| invalid("无法识别 EPUB 中的文本编码。"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("无法识别 EPUB 中的文本编码。"))
}

fn attribute(element: &BytesStart<'_>, key: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attr| attr.key.as_ref() == key)
        .and_then(|attr| attr.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|value| value.into_owned())
}

fn archive_path(package: &str, href: &str) -> io::Result<String> {
    let href = href.split(['#', '?']).next().unwrap_or_default();
    let mut decoded = Vec::with_capacity(href.len());
    let mut bytes = href.bytes();
    while let Some(byte) = bytes.next() {
        let hex = |b: u8| (b as char).to_digit(16).map(|digit| digit as u8);
        if byte == b'%' {
            let value = bytes
                .next()
                .and_then(hex)
                .zip(bytes.next().and_then(hex))
                .ok_or_else(|| invalid("EPUB 资源地址无效。"))?;
            decoded.push(value.0 * 16 + value.1);
        } else {
            decoded.push(byte);
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| invalid("EPUB 资源地址无效。"))?;
    let decoded = decoded.as_str();
    if decoded.starts_with('/') || decoded.contains([':', '\\', '\0']) || decoded.is_empty() {
        return Err(invalid("EPUB 章节必须是书内文件。"));
    }
    let mut path: Vec<_> = package
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').collect())
        .unwrap_or_default();
    for segment in decoded.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                path.pop()
                    .ok_or_else(|| invalid("EPUB 资源地址超出书内路径。"))?;
            }
            _ => path.push(segment),
        }
    }
    Ok(path.join("/"))
}

pub(super) fn extract(path: &Path) -> io::Result<(Option<String>, String, Vec<Chapter>)> {
    let mut zip = ZipArchive::new(File::open(path)?).map_err(invalid)?;
    let container = member(&mut zip, "META-INF/container.xml", 1024 * 1024)?;
    let mut reader = Reader::from_str(&container);
    let package = loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(element) | Event::Empty(element)
                if element.local_name().as_ref() == "rootfile" =>
            {
                if let Some(path) = attribute(&element, "full-path") {
                    break path;
                }
            }
            Event::Eof => return Err(invalid("EPUB 缺少内容清单。")),
            _ => {}
        }
    };
    let package = archive_path("", &package)?;
    let manifest = member(&mut zip, &package, 8 * 1024 * 1024)?;
    let (title, spine) = reading_order(&manifest)?;
    let mut text = String::new();
    let mut chapters = Vec::new();
    let mut remaining = MAX_BOOK_BYTES;
    for href in spine {
        let name = archive_path(&package, &href)?;
        let source = member(&mut zip, &name, remaining)?;
        remaining = remaining
            .checked_sub(source.len() as u64)
            .ok_or_else(|| invalid("EPUB 解包后的正文超过 128 MB。"))?;
        let (heading, body) = page_text(&source)?;
        if body.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(&body);
        text.push_str("\n\n");
        chapters.push(Chapter {
            title: heading.unwrap_or_else(|| format!("第 {} 节", chapters.len() + 1)),
            bytes: start..text.len(),
        });
    }
    Ok((title, text, chapters))
}

fn reading_order(manifest: &str) -> io::Result<(Option<String>, Vec<String>)> {
    let mut reader = Reader::from_str(manifest);
    let mut files = HashMap::new();
    let mut spine = Vec::new();
    let mut title = String::new();
    let mut in_title = false;
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(element) | Event::Empty(element) => match element.local_name().as_ref() {
                "title" if title.is_empty() => in_title = true,
                "item" => {
                    if let (Some(id), Some(href), Some(kind)) = (
                        attribute(&element, "id"),
                        attribute(&element, "href"),
                        attribute(&element, "media-type"),
                    ) {
                        if matches!(kind.as_str(), "application/xhtml+xml" | "text/html") {
                            files.insert(id, href);
                        }
                    }
                }
                "itemref" if attribute(&element, "linear").as_deref() != Some("no") => {
                    if let Some(id) = attribute(&element, "idref") {
                        spine.push(id);
                    }
                }
                _ => {}
            },
            Event::End(element) if element.local_name().as_ref() == "title" => in_title = false,
            Event::Text(value) if in_title => title.push_str(&value.xml10_content()),
            Event::GeneralRef(value) if in_title => push_entity(&mut title, &value)?,
            Event::Eof => break,
            _ => {}
        }
    }
    let ordered: Vec<_> = spine
        .into_iter()
        .filter_map(|id| files.get(&id).cloned())
        .collect();
    if ordered.is_empty() {
        return Err(invalid("EPUB 的阅读顺序中没有可读取的文本章节。"));
    }
    Ok((
        (!title.trim().is_empty()).then(|| title.trim().to_owned()),
        ordered,
    ))
}

fn page_text(source: &str) -> io::Result<(Option<String>, String)> {
    let mut reader = Reader::from_str(source);
    let mut lines = String::new();
    let mut buffer = String::new();
    let mut title = None;
    let mut body = false;
    let mut skip = 0;
    let mut heading = false;
    let block = |name: &str| {
        matches!(
            name,
            "p" | "br"
                | "div"
                | "li"
                | "blockquote"
                | "section"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        )
    };
    let hidden = |name: &str| matches!(name, "script" | "style" | "nav" | "rt" | "rp" | "svg");
    let flush = |out: &mut String, paragraph: &mut String| {
        let line = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
        paragraph.clear();
    };
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(element) => {
                let name = element.local_name();
                let name = name.as_ref();
                if name == "body" {
                    body = true;
                }
                if !body {
                    continue;
                }
                if skip > 0 || hidden(name) {
                    skip += 1;
                    continue;
                }
                if block(name) {
                    flush(&mut lines, &mut buffer);
                    heading = matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
                }
            }
            Event::Empty(element) if body && skip == 0 && block(element.local_name().as_ref()) => {
                flush(&mut lines, &mut buffer)
            }
            Event::End(element) if body => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let name = element.local_name();
                let name = name.as_ref();
                if block(name) || name == "body" {
                    if heading && title.is_none() && !buffer.trim().is_empty() {
                        title = Some(buffer.split_whitespace().collect::<Vec<_>>().join(" "));
                    }
                    flush(&mut lines, &mut buffer);
                    heading = false;
                }
                if name == "body" {
                    break;
                }
            }
            Event::Text(value) if body && skip == 0 => buffer.push_str(&value.xml10_content()),
            Event::CData(value) if body && skip == 0 => buffer.push_str(&value.xml10_content()),
            Event::GeneralRef(value) if body && skip == 0 => push_entity(&mut buffer, &value)?,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((title, lines.trim().to_owned()))
}

fn push_entity(out: &mut String, value: &quick_xml::events::BytesRef<'_>) -> io::Result<()> {
    if let Some(ch) = value.resolve_char_ref().map_err(invalid)? {
        out.push(ch);
    } else {
        let name = value.xml10_content();
        if let Some(text) = quick_xml::escape::resolve_predefined_entity(&name) {
            out.push_str(text);
        } else {
            out.push('&');
            out.push_str(&name);
            out.push(';');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_spine_order_and_keeps_only_readable_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("测试.epub");
        let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in [
            (
                "META-INF/container.xml",
                r#"<container><rootfiles><rootfile full-path="Text/package.opf"/></rootfiles></container>"#,
            ),
            (
                "Text/package.opf",
                r#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>灯&amp;潮</dc:title></metadata><manifest><item id="later" href="later.xhtml" media-type="application/xhtml+xml"/><item id="first" href="start%20here.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="first"/><itemref idref="later"/></spine></package>"#,
            ),
            (
                "Text/start here.xhtml",
                "<html><head><title>头部不进入正文</title></head><body><h1>第一章</h1><p>她关灯，听见&lt;潮声&gt;。</p><script>代码不进入正文</script></body></html>",
            ),
            (
                "Text/later.xhtml",
                "<html><body><h1>第二章</h1><p>他拉着手推车。</p></body></html>",
            ),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let book = super::super::read_book(&path).unwrap();
        assert_eq!(book.title, "灯&潮");
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[1].title, "第二章");
        assert!(book.text[book.chapters[0].bytes.clone()].contains("她关灯，听见<潮声>。"));
        assert!(!book.text.contains("不进入正文"));
        assert!(archive_path("Text/package.opf", "../../outside.xhtml").is_err());
    }
}

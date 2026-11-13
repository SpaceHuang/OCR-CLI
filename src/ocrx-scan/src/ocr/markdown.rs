//! bbox placeholder replacement and image-link validation (plan §8.10).
//!
//! Copied verbatim from the legacy `ocrx-ovisocr2` module (T037); the
//! legacy originals stay until T087.

use std::path::Path;

pub(crate) fn image_link_is_droppable(relative: &str, root: &Path) -> bool {
    !relative.starts_with("images/page-")
        || relative.contains("..")
        || !root.join(relative).is_file()
}

/// Extracts the `images/…` path (if any) from a scanned construct.
pub(crate) fn images_path_inside(text: &str) -> Option<(&str, usize, usize)> {
    let start = text.find("images/")?;
    let end = text[start..]
        .find(|character: char| {
            character.is_whitespace() || character == ')' || character == '"' || character == '>'
        })
        .map_or(text.len(), |offset| start + offset);
    Some((&text[start..end], start, end))
}

/// Q4 (quality plan §4.4): degrades invalid/missing bbox image links from a
/// page-level failure to a link-level removal. Returns the cleaned markdown
/// and the number of dropped links; each drop raises the
/// `invalid_image_link_dropped` warning at the call site.
pub(crate) fn drop_invalid_markdown_image_links(
    markdown: &str,
    output_root: impl AsRef<Path>,
) -> (String, usize) {
    let root = output_root.as_ref();
    let mut dropped = 0;

    // Pass A: `<img …>` tags carrying an images/ path.
    let mut without_tags = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find("<img") {
        let Some(tag_end_rel) = rest[start..].find('>') else {
            break;
        };
        without_tags.push_str(&rest[..start]);
        let tag_end = start + tag_end_rel + 1;
        let tag = &rest[start..tag_end];
        match images_path_inside(tag) {
            Some((relative, _, _)) if image_link_is_droppable(relative, root) => {
                dropped += 1; // tag removed
            }
            _ => without_tags.push_str(tag),
        }
        rest = &rest[tag_end..];
    }
    without_tags.push_str(rest);

    // Pass B: `![alt](images/…)` markdown constructs.
    let mut without_markdown_links = String::with_capacity(without_tags.len());
    let mut rest = without_tags.as_str();
    while let Some(bang) = rest.find("![") {
        let Some(paren_rel) = rest[bang..].find("](") else {
            without_markdown_links.push_str(&rest[..bang + 2]);
            rest = &rest[bang + 2..];
            continue;
        };
        let path_start = bang + paren_rel + 2;
        let Some(close_rel) = rest[path_start..].find(')') else {
            without_markdown_links.push_str(&rest[..path_start]);
            rest = &rest[path_start..];
            continue;
        };
        let construct_end = path_start + close_rel + 1;
        let path = &rest[path_start..path_start + close_rel];
        without_markdown_links.push_str(&rest[..bang]);
        if path.starts_with("images/") && image_link_is_droppable(path, root) {
            dropped += 1; // construct removed
        } else {
            without_markdown_links.push_str(&rest[bang..construct_end]);
        }
        rest = &rest[construct_end..];
    }
    without_markdown_links.push_str(rest);

    // Pass C: bare `images/…` tokens that survived both construct forms.
    let mut output = String::with_capacity(without_markdown_links.len());
    let mut rest = without_markdown_links.as_str();
    while let Some(start) = rest.find("images/") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let end = candidate
            .find(|character: char| {
                character.is_whitespace() || character == ')' || character == '"'
            })
            .unwrap_or(candidate.len());
        let relative = &candidate[..end];
        if image_link_is_droppable(relative, root) {
            dropped += 1; // token removed
        } else {
            output.push_str(relative);
        }
        rest = &candidate[end..];
    }
    output.push_str(rest);

    (output, dropped)
}
pub(crate) fn replace_bbox_images(raw: &str, page: u32, count: usize) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    let mut index = 0;
    while let Some(start) = rest.find("<img") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(end) = candidate.find('>') else {
            output.push_str(candidate);
            return output;
        };
        let tag = &candidate[..=end];
        if tag.contains("bbox_") && index < count {
            output.push_str(&format!(
                "<img src=\"images/page-{page:04}-figure-{figure:03}.png\" />",
                figure = index + 1
            ));
            index += 1;
        } else if tag.contains("bbox_") {
            // Invalid or excess model placeholders never become filesystem paths.
        } else {
            output.push_str(tag);
        }
        rest = &candidate[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_bbox_placeholder_cannot_escape_images_directory() {
        assert_eq!(
            replace_bbox_images("x <img src=\"../../secret\"> y", 1, 0),
            "x <img src=\"../../secret\"> y"
        );
        assert_eq!(
            replace_bbox_images("x <img src=\"images/bbox_3_3_3_3.jpg\"> y", 1, 0),
            "x  y"
        );
    }

    #[test]
    fn invalid_and_missing_image_links_are_dropped_not_page_fatal() {
        let root = std::env::temp_dir().join(format!("ocrx-ovis-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("images")).unwrap();
        // Missing file: the whole image construct is dropped, body survives.
        let (markdown, dropped) = drop_invalid_markdown_image_links(
            "before ![figure](images/page-0001-figure-001.png) after",
            &root,
        );
        assert_eq!(markdown, "before  after");
        assert_eq!(dropped, 1);
        // Path traversal: dropped (rejected by removal, never passed through).
        let (markdown, dropped) =
            drop_invalid_markdown_image_links("![x](images/../secret) 正文", &root);
        assert_eq!(markdown, " 正文");
        assert_eq!(dropped, 1);
        // Existing file: link kept byte for byte.
        std::fs::write(root.join("images/page-0001-figure-001.png"), b"png").unwrap();
        let (markdown, dropped) =
            drop_invalid_markdown_image_links("![figure](images/page-0001-figure-001.png)", &root);
        assert_eq!(markdown, "![figure](images/page-0001-figure-001.png)");
        assert_eq!(dropped, 0);
        // img-tag form with a traversal path is dropped as a whole tag.
        let (markdown, dropped) =
            drop_invalid_markdown_image_links("a <img src=\"images/../evil.png\" /> b", &root);
        assert_eq!(markdown, "a  b");
        assert_eq!(dropped, 1);
        // Mixed links: only the bad one is removed, granularity stays per link.
        let (markdown, dropped) = drop_invalid_markdown_image_links(
            "![good](images/page-0001-figure-001.png) ![bad](images/page-9999-figure-009.png)",
            &root,
        );
        assert_eq!(markdown, "![good](images/page-0001-figure-001.png) ");
        assert_eq!(dropped, 1);
        // Prefix violations that are not images/page- links are dropped too.
        let (markdown, dropped) =
            drop_invalid_markdown_image_links("看这个 images/other/stuff.png", &root);
        assert_eq!(markdown, "看这个 ");
        assert_eq!(dropped, 1);
        let _ = std::fs::remove_dir_all(root);
    }
}

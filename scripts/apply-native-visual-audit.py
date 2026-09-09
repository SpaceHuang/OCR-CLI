#!/usr/bin/env python3
"""Apply the read-only visual audit findings to the review-only candidate."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "assets/regression/pdf-native-candidate-draft-v0.1"

AUDIT = {
    1: ("image", "scanned_cover", "不允许 native；封面、书脊、主图和水印均为视觉内容。", ["书脊旋转文字 bbox", "封面主图 bbox", "水印 bbox"]),
    2: ("empty", "empty_or_watermark", "不进入 native；源页为空白，仅有下载站水印。", ["空白页判定", "水印 bbox"]),
    3: ("image", "scanned_cover", "不允许 native；反面封面含图片、条码、ISBN 和定价。", ["条码/ISBN 小字", "图片区域", "水印"]),
    4: ("mixed", "scanned_chart_formula", "不允许 native；公式、图表和扫描正文混合，且底部正文截断。", ["公式精确转写", "图表标签/bbox", "底部截断", "水印"]),
    5: ("mixed", "scanned_table", "不允许 native；表格是扫描视觉内容，候选表格结构不完整。", ["表头和数据", "表格 bbox", "脚注", "水印"]),
    6: ("mixed", "scanned_formula", "不允许 native；公式和扫描正文混合，候选公式内容缺失。", ["三个恒等式", "底部续文", "水印"]),
    7: ("mixed", "scanned_two_column", "不允许 native；扫描双栏习题页。", ["双栏阅读顺序", "题号/小项", "水印"]),
    8: ("mixed", "scanned_text", "不允许 native；扫描正文页，底部续文和水印未完整表达。", ["标题 bbox", "正文续接", "页码/水印"]),
    9: ("mixed", "scanned_image_text", "不允许 native；正文、图片和图注混合。", ["图片 bbox", "图注小字", "阅读顺序", "水印"]),
    10: ("mixed", "scanned_text_excerpt", "不允许 native；扫描正文/新闻摘录页。", ["项目符号", "新闻标题", "底部续文", "水印"]),
    11: ("empty", "empty_or_watermark", "不进入 native；源页为空白并含淡色水印。", ["淡色中央水印", "两个水印 bbox"]),
    12: ("mixed", "scanned_image_text", "不允许 native；正文、图片和图注混合，底部段落截断。", ["图片/图注 bbox", "底部截断", "页码/水印"]),
    13: ("mixed", "scanned_chart", "不允许 native；图表和扫描正文混合。", ["坐标/标签", "两幅图 bbox", "图注续文", "水印"]),
    14: ("mixed", "truncated_scanned_text", "不允许 native；含截断正文和视觉装饰。", ["截断正文", "装饰符号", "页码/水印"]),
    15: ("mixed", "scanned_two_column_index", "不允许 native；扫描双栏索引页。", ["索引条目", "双栏顺序", "页码/水印"]),
    16: ("text", "native_candidate_pending_pdfium", "暂不允许 native；视觉上以文字为主，但必须补 PDFium 证据和 L2 行/块结构。", ["每行标签/值边界", "L2 表格式结构", "PDFium 字符/bbox", "source_order"]),
    17: ("mixed", "scanned_visual_text", "不允许 native；正文和大面积装饰视觉内容混合。", ["海鸥/卷页 visual region", "标题/正文 bbox"]),
    18: ("empty", "empty_page", "不进入 native；视觉上为空白页，应走 empty 路由。", ["空白页和边缘标记"]),
    19: ("mixed", "scanned_photo_formula_table", "不允许 native；照片、公式、表格和正文混合。", ["公式", "表格单元格", "照片 bbox", "章节提示"]),
    20: ("mixed", "scanned_exercises", "不允许 native；公式、图标和习题混合，候选页眉顺序可疑。", ["公式", "页眉 source_order", "图标 bbox", "阅读顺序"]),
    21: ("mixed", "scanned_formula_text", "暂不允许 native；公式内容缺失且页面为混合内容。", ["四个公式", "答行", "变量下标", "L2 分块"]),
    22: ("mixed", "scanned_geometry_formula", "不允许 native；几何图形和公式混合。", ["公式", "四个练习图", "图文顺序"]),
    23: ("mixed", "scanned_geometry", "不允许 native；图形、公式和底部区域 bbox 需修正。", ["三组图形", "底部 bbox", "公式", "source_order"]),
    24: ("mixed", "scanned_chart", "不允许 native；柱状图和正文混合。", ["坐标轴/柱高", "标签", "图表边界"]),
    25: ("mixed", "scanned_photo_formula", "不允许 native；射击靶、计算器和公式混合。", ["计算器区域", "按键序列", "统计公式"]),
    26: ("mixed", "scanned_probability_diagram", "不允许 native；插图、公式和流程图混合。", ["概率公式", "流程图节点", "流程方向", "视觉区域"]),
    27: ("mixed", "scanned_geometry_formula", "不允许 native；主图、公式和彩色栏目混合。", ["底部面板 bbox", "栏目图标", "两条公式", "主图标签"]),
    28: ("text", "native_candidate_pending_pdfium", "暂不允许 native；视觉上以文字为主，但必须补 PDFium 证据、类别和 L2 结构。", ["类别修正", "逐块 PDFium bbox", "source_order", "全文精确性"]),
    29: ("empty", "empty_page", "不进入 native；视觉上为空白页，应走 empty 路由。", ["空白页确认"]),
    30: ("image", "scanned_spread_cover", "不允许 native；双页展开封面，候选遗漏大幅照片且标题 bbox 位置错误。", ["正面照片", "标题 bbox", "条码/环保标志", "双页阅读顺序"]),
}

def main() -> None:
    if len(AUDIT) != 30:
        raise RuntimeError("audit must cover exactly 30 pages")
    report = {"schema_version": 1, "status": "review_complete_candidate_not_accepted", "pages": []}
    for page_number in range(1, 31):
        path = TARGET / "truth" / f"page-{page_number:04}.json"
        data = json.loads(path.read_text())
        page_type, reason, finding, review_fields = AUDIT[page_number]
        for block in data.get("blocks", []):
            if "source_order" in block:
                block["source_order"] = int(block["source_order"]) + 1
            block.setdefault("source", "pdf_text")
            block.setdefault("direction", "unknown")
            block.setdefault("rotation_degrees", None)
            block.setdefault("l2_role", "unknown")
        candidate = data["native_candidate"]
        candidate.update({
            "page_type": page_type,
            "confidence": "low" if page_type != "text" else "medium",
            "planned_method": "empty" if page_type == "empty" else "ocr" if page_type != "text" else "native_text",
            "actual_method": None,
            "reason": reason,
            "allow_native": False if page_type != "text" else None,
            "review_status": "needs_human_review",
            "visual_audit_status": "reviewed",
            "visual_finding": finding,
            "review_fields": review_fields,
            "pdfium_evidence": None,
        })
        data["visual_audit"] = {
            "reviewed": True,
            "basis": "source_pdf_rendered_page_visual_inspection",
            "native_decision": "reject" if page_type != "text" else "pending_pdfium_and_human_acceptance",
            "finding": finding,
            "fields_requiring_review": review_fields,
        }
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
        report["pages"].append({
            "page_number": page_number,
            "page_type": page_type,
            "reason": reason,
            "native_decision": "reject" if page_type != "text" else "pending",
            "finding": finding,
            "fields_requiring_review": review_fields,
        })
    (TARGET / "review" / "visual-audit.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    summary = [
        "# Visual audit summary",
        "",
        "Status: complete visual review of 30/30 candidate pages; this remains a review-only draft.",
        "",
        "No page is accepted as `allow_native=true`. Pages 1-15 are scanned/image-heavy source pages; pages 18, 29 are empty; pages 16 and 28 are only conditional native candidates pending PDFium evidence and L2 review.",
        "",
        "| Page type | Pages | Native decision |",
        "| --- | --- | --- |",
        "| image/mixed | 1-15, 17, 19-27, 30 | reject |",
        "| empty | 2, 11, 18, 29 | route as empty |",
        "| text candidate | 16, 28 | pending PDFium + human acceptance |",
        "",
        "All page-level findings and required review fields are in `review/visual-audit.json` and each `truth/page-NNNN.json`. Source orders were normalized to start at 1; no page was marked native-success without PDFium evidence.",
    ]
    (TARGET / "review" / "visual-audit-summary.md").write_text("\n".join(summary) + "\n")

if __name__ == "__main__":
    main()

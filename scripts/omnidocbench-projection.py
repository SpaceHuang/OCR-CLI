#!/usr/bin/env python3
"""OmniDocBench 基线的官方口径收益投影（质量优化方案 v1.2 的唯一数字来源）。

评审 BLK-1（docs/review/omnidocbench-quality-optimization-development-plan.md）：
Overall 三个分量的聚合粒度不同，投影必须全部按官方口径计算——

- 文本：逐页 (1 - Edit_dist) 均值           （reports/..._text_block_per_page_edit.json）
- 公式：全部公式样本 CDM 均值               （reports/..._display_formula_per_sample_CDM.json）
- 表格：全部表格样本 TEDS 均值              （reports/..._table_per_table_TEDS.json）
- Overall = ((1 - 文本) + 公式 + 表格) / 3 × 100

投影模型：坏页/坏样本的分量值线性移向对应"正常均值"（θ = 缺口关闭率，θ=1 即全部
修复到正常均值）。坏页集合取 predictions/<profile>/export-report.json 的 anomalies
（degenerated/truncated 标记）经 pdfs/mapping.json 的 stem→image_path 连接；缺失页
（recognition_failed）单列。正常均值 = 官方明细中非坏页/非坏样本的均值，随坏页集合
定义而变，因此投影对坏页账本口径敏感——口径由本脚本冻结。

用法：
  python scripts/omnidocbench-projection.py --benchmark-root benchmarks/omnidocbench \
      [--profile balanced] [--theta 0 0.25 0.5 0.75 1]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark-root", default="benchmarks/omnidocbench")
    parser.add_argument("--profile", default="balanced")
    parser.add_argument("--theta", type=float, nargs="*", default=[0.0, 0.25, 0.5, 0.75, 1.0])
    args = parser.parse_args()

    root = Path(args.benchmark_root)
    profile = args.profile
    export = load_json(root / "predictions" / profile / "export-report.json")
    mapping = load_json(root / "pdfs" / "mapping.json")
    stem2img = {p["stem"]: p["image_path"] for p in mapping["pages"]}

    degenerated = {e["stem"] for e in export["anomalies"] if e.get("degenerated")}
    truncated = {e["stem"] for e in export["anomalies"] if e.get("truncated")}
    missing = set(export.get("missing", []))

    text = load_json(root / "reports" / f"omnidocbench-{profile}-all_quick_match_text_block_per_page_edit.json")
    cdm = load_json(root / "reports" / f"omnidocbench-{profile}-all_quick_match_display_formula_per_sample_CDM.json")
    teds = load_json(root / "reports" / f"omnidocbench-{profile}-all_quick_match_table_per_table_TEDS.json")

    def page_of(sample_key: str) -> str:
        return sample_key.split("_[")[0]

    text_avg = sum(text.values()) / len(text)
    cdm_avg = sum(cdm.values()) / len(cdm)
    teds_avg = sum(v["TEDS"] for v in teds.values()) / len(teds)
    current = ((1 - text_avg) + cdm_avg + teds_avg) / 3 * 100

    def imgs(stems):
        return {stem2img[s] for s in stems if s in stem2img}

    scenarios = {
        f"Q1 语料（退化页 {len(degenerated)} 页，含 {len(degenerated & truncated)} 交集）": imgs(degenerated),
        f"全部坏页（{len(degenerated | truncated)} 页）+ 缺失 {len(missing)} 页": imgs(degenerated | truncated | missing),
    }

    print(f"profile={profile}  text n={len(text)}  cdm n={len(cdm)}  teds n={len(teds)}")
    print(f"官方分量：text(1-edit)={1-text_avg:.4f}  CDM(all)={cdm_avg:.4f}  TEDS(all)={teds_avg:.4f}")
    print(f"当前 Overall = {current:.4f}")
    print(f"坏页账本：|D|={len(degenerated)} |T|={len(truncated)} |D∩T|={len(degenerated & truncated)} "
          f"纯T={len(truncated - degenerated)} 缺失={len(missing)}")

    for label, bad_pages in scenarios.items():
        b_tx = [text[k] for k in text if k in bad_pages]
        o_tx = [v for k, v in text.items() if k not in bad_pages]
        b_c = [v for k, v in cdm.items() if page_of(k) in bad_pages]
        o_c = [v for k, v in cdm.items() if page_of(k) not in bad_pages]
        b_t = [v["TEDS"] for k, v in teds.items() if page_of(k) in bad_pages]
        o_t = [v["TEDS"] for k, v in teds.items() if page_of(k) not in bad_pages]
        if not (b_tx or b_c or b_t):
            print(f"\n[{label}] 无坏页匹配，跳过")
            continue
        m_tx, m_c, m_t = (sum(x) / len(x) for x in (o_tx, o_c, o_t))

        def scenario(theta: float) -> float:
            nt = (sum(o_tx) + sum(x + theta * (m_tx - x) for x in b_tx)) / len(text)
            nc = (sum(o_c) + sum(x + theta * (m_c - x) for x in b_c)) / len(cdm)
            nd = (sum(o_t) + sum(x + theta * (m_t - x) for x in b_t)) / len(teds)
            return ((1 - nt) + nc + nd) / 3 * 100

        full = scenario(1.0)
        gap = full - current
        print(f"\n[{label}] 文本坏页={len(b_tx)} 公式坏样本={len(b_c)} 表格坏样本={len(b_t)}")
        print(f"  正常均值：text={1-m_tx:.4f} CDM={m_c:.4f} TEDS={m_t:.4f}  可修复缺口={gap:.4f}")
        for theta in args.theta:
            value = scenario(theta)
            extra = ""
            if theta < 1 and gap > 0:
                extra = f"  （Overall 88 需 θ={min(1.0, (88 - current) / gap):.2f}，85 需 θ={min(1.0, (85 - current) / gap):.2f}）"
            print(f"  θ={theta:.2f} → Overall {value:.4f}{extra}")


if __name__ == "__main__":
    main()

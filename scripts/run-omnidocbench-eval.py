#!/usr/bin/env python3
"""Run the official OmniDocBench evaluation for one prediction set in Docker.

Wraps the reference image documented by OmniDocBench
(`ghcr.io/zeng-weijun/omnidocbench-eval:repro-ubuntu2204`, Python 3.10 +
TeX Live 2025 + ImageMagick 7.1.1-47 + Ghostscript 9.55.0):

- writes `eval-configs/<name>.yaml` with the end2end metric set
  (text Edit_dist, formula Edit_dist+CDM, table TEDS+Edit_dist, reading
  order) and `quick_match`;
- bind-mounts the GT JSON read-only, the prediction folder read-only, the
  host `reports/` as the container's result directory, and the configs;
- runs `pdf_validation.py --config ...` and streams the log to
  `reports/<name>.log`.

Metric result files land in `reports/` as
`<predictions-basename>_quick_match_metric_result.json` (the toolkit's
`build_save_name`), alongside run/stage summaries.

Splits are done via the toolkit's `dataset.filter` (matches
`page_attribute` exactly): `--filter language=english` etc.

Usage:
  run-omnidocbench-eval.py --benchmark-root benchmarks/omnidocbench \
      --gt dataset/OmniDocBench.json --predictions predictions/balanced \
      --name omnidocbench-balanced [--workers 8] [--filter language=english]
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

DEFAULT_IMAGE = "ghcr.io/zeng-weijun/omnidocbench-eval:repro-ubuntu2204"


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--benchmark-root", type=Path, required=True)
    parser.add_argument("--gt", type=Path, required=True,
                        help="GT JSON (host path, mounted read-only)")
    parser.add_argument("--predictions", type=Path, required=True,
                        help="prediction markdown folder (host path)")
    parser.add_argument("--name", required=True,
                        help="config/log label, e.g. omnidocbench-balanced-en")
    parser.add_argument("--image", default=DEFAULT_IMAGE)
    parser.add_argument("--workers", type=int, default=8,
                        help="match/cdm/teds workers (CDM needs ~1GB RAM each)")
    parser.add_argument("--filter", action="append", default=[],
                        help="page_attribute filter key=value, repeatable")
    parser.add_argument("--dry-run", action="store_true",
                        help="write the YAML and print the docker command only")
    return parser.parse_args(argv)


def build_yaml(name: str, workers: int, filters: dict) -> str:
    filter_block = ""
    if filters:
        entries = "\n".join(f"      {k}: {json.dumps(v)}" for k, v in filters.items())
        filter_block = f"    filter:\n{entries}\n"
    return f"""end2end_eval:
  metrics:
    text_block:
      metric:
      - Edit_dist
    display_formula:
      metric:
      - Edit_dist
      - CDM
      cdm_workers: {workers}
    table:
      metric:
      - TEDS
      - Edit_dist
      teds_workers: {workers}
    reading_order:
      metric:
      - Edit_dist
  dataset:
    dataset_name: end2end_dataset
    ground_truth:
      data_path: /workspace/gt/OmniDocBench.json
    prediction:
      data_path: /workspace/data_md/{name}
    match_method: quick_match
    match_workers: {workers}
    quick_match_truncated_timeout_sec: 300
    match_timeout_sec: 420
    timeout_fallback_max_chunk_span: 10
    timeout_fallback_order_penalty: 0.10
{filter_block}"""


def container_workdir(image: str) -> str:
    probe = subprocess.run(
        ["docker", "image", "inspect", "--format", "{{.Config.WorkingDir}}", image],
        capture_output=True, text=True, check=False)
    workdir = probe.stdout.strip() if probe.returncode == 0 else ""
    return workdir or "/workspace"


def main(argv=None) -> int:
    args = parse_args(argv)
    root = args.benchmark_root.resolve()
    gt = args.gt.resolve()
    predictions = args.predictions.resolve()
    if not gt.is_file():
        print(f"ERROR: GT JSON not found: {gt}", file=sys.stderr)
        return 2
    if not predictions.is_dir():
        print(f"ERROR: prediction folder not found: {predictions}", file=sys.stderr)
        return 2

    configs_dir = root / "eval-configs"
    configs_dir.mkdir(parents=True, exist_ok=True)
    (root / "reports").mkdir(parents=True, exist_ok=True)

    filters = {}
    for item in args.filter:
        key, _, value = item.partition("=")
        filters[key.strip()] = value.strip()

    config_path = configs_dir / f"{args.name}.yaml"
    config_path.write_text(build_yaml(args.name, args.workers, filters), encoding="utf-8")

    workdir = container_workdir(args.image)
    inner = (
        f"cd {workdir} && "
        f"python pdf_validation.py --config configs-custom/{args.name}.yaml "
        f"2>&1 | tee /workspace/result/{args.name}.log"
    )
    docker_cmd = [
        "docker", "run", "--rm", "--entrypoint", "bash",
        "-v", f"{gt}:/workspace/gt/OmniDocBench.json:ro",
        "-v", f"{predictions}:/workspace/data_md/{args.name}:ro",
        "-v", f"{root / 'reports'}:/workspace/result",
        "-v", f"{configs_dir}:/workspace/configs-custom:ro",
        args.image, "-c", inner,
    ]

    if args.dry_run:
        print(config_path)
        print(" ".join(docker_cmd))
        return 0

    print(f"[eval:{args.name}] config={config_path}")
    completed = subprocess.run(docker_cmd)
    if completed.returncode != 0:
        print(f"ERROR: docker run exited {completed.returncode} for {args.name}",
              file=sys.stderr)
        return completed.returncode

    # The toolkit derives result-file names from the mounted prediction folder
    # basename, so the mount point carries the run name to avoid collisions.
    save_name = args.name + "_quick_match"
    metric_result = root / "reports" / f"{save_name}_metric_result.json"
    print(f"[eval:{args.name}] done, metric result: {metric_result}")
    if metric_result.is_file():
        print(metric_result.read_text(encoding="utf-8")[:4000])
    return 0


if __name__ == "__main__":
    sys.exit(main())

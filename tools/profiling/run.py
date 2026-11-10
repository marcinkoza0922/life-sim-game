#!/usr/bin/env python3
"""Build once; run serial seeded repetitions; preserve raw JSONL and summarize medians."""
import argparse
import json
import os
import platform
import shutil
from datetime import datetime, timezone
from pathlib import Path
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--out', type=Path, default=ROOT / 'out/profiling.jsonl')
p.add_argument('--repeats', type=int, default=3)
p.add_argument('--iterations', type=int, default=1440)
p.add_argument('--cpu', type=int, help='pin benchmark processes to this allowed Linux logical CPU')
a = p.parse_args()
if a.repeats < 1 or a.iterations < 1:
    p.error('repeats and iterations must be positive')
if a.cpu is not None:
    if not hasattr(os, 'sched_getaffinity') or a.cpu not in os.sched_getaffinity(0):
        p.error('--cpu must be an allowed Linux logical CPU')
    if shutil.which('taskset') is None:
        p.error('--cpu requires taskset')
benchmark_prefix = [] if a.cpu is None else ['taskset', '-c', str(a.cpu)]
subprocess.run(['cargo', 'build', '-q', '--offline', '--profile', 'profiling', '-p', 'sim', '--example', 'profile', '--features', 'profiling'], cwd=ROOT, check=True)
target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
if not target.is_absolute():
    target = ROOT / target
binary = target / 'profiling/examples/profile'
a.out.parent.mkdir(parents=True, exist_ok=True)
metadata = {
    'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
    'dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip()),
    'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
    'platform': platform.platform(),
    'started_utc': datetime.now(timezone.utc).isoformat(),
    'repeats': a.repeats, 'iterations': a.iterations, 'benchmark_cpu': a.cpu,
}
a.out.with_suffix('.meta.json').write_text(json.dumps(metadata, indent=2) + '\n')
with a.out.open('w') as output:
    for mode, counts in [('tick', [4, 16, 64, 128]), ('autonomy', [4, 64, 128]), ('nav', [16, 32, 64, 128]), ('nav-house', [4, 64, 128]), ('save', [4, 64, 128])]:
        for count in counts:
            rows = []
            for seed in range(1, a.repeats + 1):
                iterations = a.iterations if mode == 'tick' else min(a.iterations, 200)
                raw = subprocess.check_output(benchmark_prefix + [str(binary), mode, str(count), str(iterations), str(seed)], cwd=ROOT, text=True)
                row = json.loads(raw)
                rows.append(row)
                output.write(json.dumps(row) + '\n')
                output.flush()
            mean = statistics.median(r['mean_us'] for r in rows)
            p99 = statistics.median(r['p99_us'] for r in rows)
            print(f'{mode:8} {count:4}: median mean {mean:10.1f} us; median p99 {p99:10.1f} us', flush=True)
            if mode == 'tick':
                systems = {name: statistics.median(r['system_mean_us'][name] for r in rows)
                           for name in rows[0]['system_mean_us']}
                for name, cost in sorted(systems.items(), key=lambda item: item[1], reverse=True)[:3]:
                    print(f'  {name}: {cost:.1f} us', flush=True)

#!/usr/bin/env python3
"""Compare median mean latency in two run.py JSONL reports (positive = slower)."""
import argparse
from collections import defaultdict
import json
import statistics
import sys


def load(path):
    groups = defaultdict(list)
    rows = {}
    with open(path) as source:
        for line in source:
            r = json.loads(line)
            groups[r['mode'], r['count']].append(r['mean_us'])
            key = (r['mode'], r['count'], r['seed'])
            if key in rows:
                raise ValueError(f'duplicate seeded case: {key}')
            rows[key] = r
    return {k: statistics.median(v) for k, v in groups.items()}, rows


p = argparse.ArgumentParser(description=__doc__)
p.add_argument('baseline')
p.add_argument('candidate')
p.add_argument('--verify-state', action='store_true',
               help='require identical seeded cases, workloads, checksums and final state fingerprints')
a = p.parse_args()
(before, old_rows), (after, new_rows) = load(a.baseline), load(a.candidate)
if a.verify_state:
    if old_rows.keys() != new_rows.keys():
        p.error('seeded cases differ between reports')
    differences = []
    for key, old in old_rows.items():
        new = new_rows[key]
        for field in ['state_hash', 'checksum', 'iterations', 'initial_population', 'final_population']:
            if field not in old or field not in new or old[field] != new[field]:
                differences.append(f'{key}: {field} differs or is missing')
    if differences:
        print('\n'.join(differences[:10]), file=sys.stderr)
        sys.exit(1)
for key in sorted(before.keys() & after.keys()):
    delta = (after[key] / before[key] - 1) * 100
    print(f'{key[0]:8} {key[1]:4}: {before[key]:10.1f} -> {after[key]:10.1f} us ({delta:+.1f}%)')
if a.verify_state:
    print(f'Replay checks match for {len(old_rows)} seeded cases.')

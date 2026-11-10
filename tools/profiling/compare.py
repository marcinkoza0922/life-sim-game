#!/usr/bin/env python3
"""Compare median mean latency in two run.py JSONL reports (positive = slower)."""
import argparse
from collections import defaultdict
import json
import statistics


def load(path):
    groups = defaultdict(list)
    with open(path) as source:
        for line in source:
            r = json.loads(line)
            groups[r['mode'], r['count']].append(r['mean_us'])
    return {k: statistics.median(v) for k, v in groups.items()}


p = argparse.ArgumentParser(description=__doc__)
p.add_argument('baseline')
p.add_argument('candidate')
a = p.parse_args()
before, after = load(a.baseline), load(a.candidate)
for key in sorted(before.keys() & after.keys()):
    delta = (after[key] / before[key] - 1) * 100
    print(f'{key[0]:8} {key[1]:4}: {before[key]:10.1f} -> {after[key]:10.1f} us ({delta:+.1f}%)')

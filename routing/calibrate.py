#!/usr/bin/env python3
"""Fit acceptance thresholds and verify them on a separate labeled corpus.
This calibrates acceptance gates, not the model's probability distribution.
"""
import argparse
import json
from pathlib import Path

LABELS=['answer','native','browser','desktop','work']

def unique_rows(report):
    rows={}
    for row in report['rows']:
        if row.get('route') and row.get('confidence') is not None:
            rows.setdefault((row['id'],row['source']),row)
    return list(rows.values())

def precision(rows):
    return sum(r['route']==r['expected'] for r in rows)/len(rows) if rows else 0

def fit(train, validation, minimum=20, required=.98):
    thresholds={};stats={}
    for source in ['semantic','laya']:
        thresholds[source]={};stats[source]={}
        for route in LABELS:
            examples=[r for r in train if r['source']==source and r['route']==route]
            candidates=sorted({max(.5,r['confidence']) for r in examples})
            threshold=2.0  # insufficient evidence: abstain
            for candidate in candidates:
                accepted=[r for r in examples if r['confidence']>=candidate]
                if len(accepted)>=minimum and precision(accepted)>=required:
                    threshold=candidate;break
            held_out=[r for r in validation if r['source']==source and r['route']==route and r['confidence']>=threshold]
            passed=len(held_out)>=minimum and precision(held_out)>=required
            thresholds[source][route]=threshold if passed else 2.0
            stats[source][route]={'held_out_accepted':len(held_out),'held_out_precision':precision(held_out) if held_out else None,'enabled':passed}
    return thresholds,stats

def main():
    p=argparse.ArgumentParser();p.add_argument('--training',type=Path,required=True);p.add_argument('--validation',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--minimum-accepted',type=int,default=20);args=p.parse_args()
    train=json.loads(args.training.read_text());validation=json.loads(args.validation.read_text())
    for report in [train,validation]:
        if not report.get('independent_user_quality_measurement'):
            p.error('calibration requires user-supplied labeled corpora; synthetic checks cannot activate routing gates')
        if report.get('confidence')!=.5:
            p.error('collect calibration candidates with evaluate.py --confidence 0.5')
        if len({r['id'] for r in report['rows']})<200:
            p.error('use at least 200 independently labeled cases per corpus')
    if {r['id'] for r in train['rows']} & {r['id'] for r in validation['rows']}:
        p.error('training and validation case IDs must be disjoint')
    thresholds,stats=fit(unique_rows(train),unique_rows(validation),args.minimum_accepted)
    args.output.write_text(json.dumps({'version':1,'source':'independent labeled corpora','thresholds':thresholds,'validation':stats},indent=2)+'\n')
    print('Wrote acceptance gates verified against a separate corpus; unsupported classes abstain.')

if __name__=='__main__':main()

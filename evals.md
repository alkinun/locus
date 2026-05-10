```
locus eval

Dataset: evals/locus.synthetic.yaml
Repo: /home/alkinunl/Desktop/locus
Queries: 100
Limit: 10
Embeddings: disabled

Overall:
  Recall@1:   21.0%
  Recall@3:   34.0%
  Recall@5:   47.0%
  Recall@10:  59.0%
  MRR:        0.31
  nDCG@5:     0.34
  nDCG@10:    0.38
  p50:        65 ms
  p95:        120 ms
  max:        160 ms

By style:
  AgentTask              n=13   R@5  69.2%   MRR 0.34   nDCG@5 0.42
  Architecture           n=3    R@5   0.0%   MRR 0.05   nDCG@5 0.00
  CasualVague            n=12   R@5  25.0%   MRR 0.08   nDCG@5 0.11
  ChangeTarget           n=14   R@5  57.1%   MRR 0.26   nDCG@5 0.32
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  38.9%   MRR 0.31   nDCG@5 0.30
  DefinitionQuestion     n=6    R@5  83.3%   MRR 0.83   nDCG@5 0.83
  FuzzyImplementation    n=14   R@5  35.7%   MRR 0.15   nDCG@5 0.19
  TestFinding            n=10   R@5 100.0%   MRR 0.92   nDCG@5 0.94
  UsageQuestion          n=9    R@5   0.0%   MRR 0.03   nDCG@5 0.00

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.14   nDCG@5 0.00
  debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  implementation         n=97   R@5  47.4%   MRR 0.31   nDCG@5 0.34
  usage                  n=1    R@5   0.0%   MRR 0.11   nDCG@5 0.00
```

---

```
locus eval

Dataset: evals/locus.synthetic.yaml
Repo: /home/alkinunl/Desktop/locus
Queries: 100
Limit: 10
Embeddings: enabled

Overall:
  Recall@1:   39.0%
  Recall@3:   63.0%
  Recall@5:   76.0%
  Recall@10:  85.0%
  MRR:        0.54
  nDCG@5:     0.58
  nDCG@10:    0.61
  p50:        108 ms
  p95:        146 ms
  max:        170 ms

By style:
  AgentTask              n=13   R@5  92.3%   MRR 0.58   nDCG@5 0.66
  Architecture           n=3    R@5  66.7%   MRR 0.18   nDCG@5 0.30
  CasualVague            n=12   R@5  58.3%   MRR 0.24   nDCG@5 0.31
  ChangeTarget           n=14   R@5  85.7%   MRR 0.56   nDCG@5 0.63
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  77.8%   MRR 0.53   nDCG@5 0.60
  DefinitionQuestion     n=6    R@5 100.0%   MRR 0.92   nDCG@5 0.94
  FuzzyImplementation    n=14   R@5  64.3%   MRR 0.52   nDCG@5 0.54
  TestFinding            n=10   R@5 100.0%   MRR 1.00   nDCG@5 1.00
  UsageQuestion          n=9    R@5  44.4%   MRR 0.28   nDCG@5 0.28

By intent:
  architecture           n=1    R@5 100.0%   MRR 0.20   nDCG@5 0.39
  debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  implementation         n=97   R@5  75.3%   MRR 0.53   nDCG@5 0.58
  usage                  n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
```

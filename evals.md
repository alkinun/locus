```
locus eval

Dataset: evals/locus.synthetic.yaml
Repo: /home/alkinunl/Desktop/locus
Queries: 100
Limit: 10
Embeddings: disabled

Overall:
  Recall@1:   19.0%
  Recall@3:   34.0%
  Recall@5:   47.0%
  Recall@10:  60.0%
  MRR:        0.30
  nDCG@5:     0.33
  nDCG@10:    0.37
  p50:        81 ms
  p95:        127 ms
  max:        147 ms

By style:
  AgentTask              n=13   R@5  53.8%   MRR 0.24   nDCG@5 0.30
  Architecture           n=3    R@5   0.0%   MRR 0.05   nDCG@5 0.00
  CasualVague            n=12   R@5  25.0%   MRR 0.08   nDCG@5 0.11
  ChangeTarget           n=14   R@5  64.3%   MRR 0.24   nDCG@5 0.34
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  38.9%   MRR 0.32   nDCG@5 0.30
  DefinitionQuestion     n=6    R@5  83.3%   MRR 0.83   nDCG@5 0.83
  FuzzyImplementation    n=14   R@5  35.7%   MRR 0.14   nDCG@5 0.18
  TestFinding            n=10   R@5 100.0%   MRR 0.93   nDCG@5 0.95
  UsageQuestion          n=9    R@5  11.1%   MRR 0.03   nDCG@5 0.04

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.14   nDCG@5 0.00
  debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  implementation         n=97   R@5  47.4%   MRR 0.30   nDCG@5 0.33
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
  Recall@1:   43.0%
  Recall@3:   63.0%
  Recall@5:   70.0%
  Recall@10:  80.0%
  MRR:        0.55
  nDCG@5:     0.58
  nDCG@10:    0.62
  p50:        130 ms
  p95:        181 ms
  max:        199 ms

By style:
  AgentTask              n=13   R@5  76.9%   MRR 0.47   nDCG@5 0.54
  Architecture           n=3    R@5  66.7%   MRR 0.22   nDCG@5 0.33
  CasualVague            n=12   R@5  50.0%   MRR 0.26   nDCG@5 0.31
  ChangeTarget           n=14   R@5  78.6%   MRR 0.63   nDCG@5 0.64
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  72.2%   MRR 0.57   nDCG@5 0.60
  DefinitionQuestion     n=6    R@5 100.0%   MRR 0.92   nDCG@5 0.94
  FuzzyImplementation    n=14   R@5  64.3%   MRR 0.57   nDCG@5 0.58
  TestFinding            n=10   R@5 100.0%   MRR 1.00   nDCG@5 1.00
  UsageQuestion          n=9    R@5  33.3%   MRR 0.32   nDCG@5 0.29

By intent:
  architecture           n=1    R@5 100.0%   MRR 0.33   nDCG@5 0.50
  debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  implementation         n=97   R@5  69.1%   MRR 0.54   nDCG@5 0.57
  usage                  n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
```

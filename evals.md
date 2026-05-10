```
locus eval

Dataset: evals/locus.synthetic.yaml
Repo: /home/alkinunl/Desktop/locus
Queries: 100
Limit: 10
Embeddings: disabled

Overall:
  Recall@1:   25.0%
  Recall@3:   37.0%
  Recall@5:   50.0%
  Recall@10:  65.0%
  MRR:        0.35
  nDCG@5:     0.37
  nDCG@10:    0.42
  p50:        122 ms
  p95:        197 ms
  max:        332 ms

By style:
  AgentTask              n=13   R@5  46.2%   MRR 0.25   nDCG@5 0.28
  Architecture           n=3    R@5  33.3%   MRR 0.12   nDCG@5 0.14
  CasualVague            n=12   R@5  41.7%   MRR 0.31   nDCG@5 0.33
  ChangeTarget           n=14   R@5  78.6%   MRR 0.44   nDCG@5 0.52
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  55.6%   MRR 0.35   nDCG@5 0.38
  DefinitionQuestion     n=6    R@5  83.3%   MRR 0.83   nDCG@5 0.83
  FuzzyImplementation    n=14   R@5  42.9%   MRR 0.41   nDCG@5 0.39
  TestFinding            n=10   R@5  30.0%   MRR 0.22   nDCG@5 0.24
  UsageQuestion          n=9    R@5  33.3%   MRR 0.29   nDCG@5 0.28

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.11   nDCG@5 0.00
  debugging              n=1    R@5 100.0%   MRR 0.25   nDCG@5 0.43
  implementation         n=97   R@5  49.5%   MRR 0.35   nDCG@5 0.37
  usage                  n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
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
  Recall@1:   27.0%
  Recall@3:   42.0%
  Recall@5:   51.0%
  Recall@10:  69.0%
  MRR:        0.38
  nDCG@5:     0.40
  nDCG@10:    0.45
  p50:        178 ms
  p95:        263 ms
  max:        436 ms

By style:
  AgentTask              n=13   R@5  53.8%   MRR 0.36   nDCG@5 0.38
  Architecture           n=3    R@5  66.7%   MRR 0.13   nDCG@5 0.26
  CasualVague            n=12   R@5  41.7%   MRR 0.28   nDCG@5 0.30
  ChangeTarget           n=14   R@5  64.3%   MRR 0.51   nDCG@5 0.52
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  50.0%   MRR 0.36   nDCG@5 0.35
  DefinitionQuestion     n=6    R@5 100.0%   MRR 0.92   nDCG@5 0.94
  FuzzyImplementation    n=14   R@5  57.1%   MRR 0.48   nDCG@5 0.50
  TestFinding            n=10   R@5  20.0%   MRR 0.22   nDCG@5 0.20
  UsageQuestion          n=9    R@5  33.3%   MRR 0.22   nDCG@5 0.23

By intent:
  architecture           n=1    R@5 100.0%   MRR 0.20   nDCG@5 0.39
  debugging              n=1    R@5 100.0%   MRR 0.20   nDCG@5 0.39
  implementation         n=97   R@5  49.5%   MRR 0.38   nDCG@5 0.39
  usage                  n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
```

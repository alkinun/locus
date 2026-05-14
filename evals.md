```
locus eval

Dataset: /home/alkinunl/Desktop/locus-eval-codebase/evals/generated.yaml
Repo: /home/alkinunl/Desktop/locus-eval-codebase
Queries: 192
Limit: 10
Embeddings: disabled
Reranker:   disabled

Overall:
  Recall@1:   9.9%
  Recall@3:   22.9%
  Recall@5:   30.7%
  Recall@10:  43.8%
  MRR:        0.19
  nDCG@5:     0.21
  nDCG@10:    0.26
  p50:        19 ms
  p95:        29 ms
  max:        43 ms

By style:
  AgentTask              n=21   R@5  33.3%   MRR 0.19   nDCG@5 0.21
  Architecture           n=2    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  Capability             n=2    R@5   0.0%   MRR 0.12   nDCG@5 0.00
  CasualVague            n=38   R@5  21.1%   MRR 0.17   nDCG@5 0.16
  ChangeTarget           n=53   R@5  20.8%   MRR 0.12   nDCG@5 0.13
  ConfigFinding          n=2    R@5  50.0%   MRR 0.25   nDCG@5 0.32
  DebuggingSymptom       n=26   R@5  57.7%   MRR 0.34   nDCG@5 0.39
  DefinitionQuestion     n=3    R@5  33.3%   MRR 0.38   nDCG@5 0.33
  DocsQuestion           n=2    R@5  50.0%   MRR 0.17   nDCG@5 0.25
  FuzzyImplementation    n=41   R@5  34.1%   MRR 0.21   nDCG@5 0.24
  UsageQuestion          n=2    R@5  50.0%   MRR 0.12   nDCG@5 0.22

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  implementation         n=189  R@5  30.7%   MRR 0.19   nDCG@5 0.21
  usage                  n=2    R@5  50.0%   MRR 0.12   nDCG@5 0.22
```

---

```
locus eval

Dataset: /home/alkinunl/Desktop/locus-eval-codebase/evals/generated.yaml
Repo: /home/alkinunl/Desktop/locus-eval-codebase
Queries: 192
Limit: 10
Embeddings: enabled
Reranker:   disabled

Overall:
  Recall@1:   37.0%
  Recall@3:   65.6%
  Recall@5:   75.0%
  Recall@10:  83.3%
  MRR:        0.53
  nDCG@5:     0.59
  nDCG@10:    0.63
  p50:        59 ms
  p95:        74 ms
  max:        126 ms

By style:
  AgentTask              n=21   R@5  76.2%   MRR 0.46   nDCG@5 0.52
  Architecture           n=2    R@5  50.0%   MRR 0.20   nDCG@5 0.22
  Capability             n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  CasualVague            n=38   R@5  52.6%   MRR 0.38   nDCG@5 0.41
  ChangeTarget           n=53   R@5  81.1%   MRR 0.58   nDCG@5 0.65
  ConfigFinding          n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  DebuggingSymptom       n=26   R@5  76.9%   MRR 0.55   nDCG@5 0.60
  DefinitionQuestion     n=3    R@5 100.0%   MRR 0.83   nDCG@5 0.88
  DocsQuestion           n=2    R@5  50.0%   MRR 0.50   nDCG@5 0.50
  FuzzyImplementation    n=41   R@5  82.9%   MRR 0.59   nDCG@5 0.67
  UsageQuestion          n=2    R@5 100.0%   MRR 0.23   nDCG@5 0.60

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.14   nDCG@5 0.00
  implementation         n=189  R@5  75.1%   MRR 0.53   nDCG@5 0.59
  usage                  n=2    R@5 100.0%   MRR 0.23   nDCG@5 0.60
```

---

```
locus eval

Dataset: /home/alkinunl/Desktop/locus-eval-codebase/evals/generated.yaml
Repo: /home/alkinunl/Desktop/locus-eval-codebase
Queries: 192
Limit: 10
Embeddings: enabled
Reranker:   enabled

Overall:
  Recall@1:   71.4%
  Recall@3:   83.3%
  Recall@5:   87.0%
  Recall@10:  94.3%
  MRR:        0.79
  nDCG@5:     0.81
  nDCG@10:    0.83
  p50:        2589 ms
  p95:        3103 ms
  max:        3259 ms

By style:
  AgentTask              n=21   R@5  81.0%   MRR 0.70   nDCG@5 0.72
  Architecture           n=2    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  Capability             n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  CasualVague            n=38   R@5  94.7%   MRR 0.86   nDCG@5 0.89
  ChangeTarget           n=53   R@5  90.6%   MRR 0.83   nDCG@5 0.85
  ConfigFinding          n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  DebuggingSymptom       n=26   R@5  80.8%   MRR 0.68   nDCG@5 0.72
  DefinitionQuestion     n=3    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  DocsQuestion           n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  FuzzyImplementation    n=41   R@5  85.4%   MRR 0.81   nDCG@5 0.80
  UsageQuestion          n=2    R@5  50.0%   MRR 0.25   nDCG@5 0.50

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  implementation         n=189  R@5  87.8%   MRR 0.80   nDCG@5 0.82
  usage                  n=2    R@5  50.0%   MRR 0.25   nDCG@5 0.50
```

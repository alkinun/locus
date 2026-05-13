```
locus eval

Dataset: /home/alkinunl/Desktop/locus-eval-codebase/evals/generated.yaml
Repo: /home/alkinunl/Desktop/locus-eval-codebase
Queries: 192
Limit: 10
Embeddings: disabled

Overall:
  Recall@1:   9.9%
  Recall@3:   22.9%
  Recall@5:   30.7%
  Recall@10:  43.8%
  MRR:        0.19
  nDCG@5:     0.21
  nDCG@10:    0.26
  p50:        17 ms
  p95:        22 ms
  max:        26 ms

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

Overall:
  Recall@1:   37.5%
  Recall@3:   65.6%
  Recall@5:   75.0%
  Recall@10:  83.3%
  MRR:        0.53
  nDCG@5:     0.59
  nDCG@10:    0.63
  p50:        58 ms
  p95:        77 ms
  max:        125 ms

By style:
  AgentTask              n=21   R@5  76.2%   MRR 0.46   nDCG@5 0.52
  Architecture           n=2    R@5  50.0%   MRR 0.20   nDCG@5 0.22
  Capability             n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  CasualVague            n=38   R@5  52.6%   MRR 0.38   nDCG@5 0.41
  ChangeTarget           n=53   R@5  81.1%   MRR 0.59   nDCG@5 0.65
  ConfigFinding          n=2    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  DebuggingSymptom       n=26   R@5  76.9%   MRR 0.55   nDCG@5 0.60
  DefinitionQuestion     n=3    R@5 100.0%   MRR 0.83   nDCG@5 0.88
  DocsQuestion           n=2    R@5  50.0%   MRR 0.50   nDCG@5 0.50
  FuzzyImplementation    n=41   R@5  82.9%   MRR 0.59   nDCG@5 0.67
  UsageQuestion          n=2    R@5 100.0%   MRR 0.23   nDCG@5 0.60

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.14   nDCG@5 0.00
  implementation         n=189  R@5  75.1%   MRR 0.54   nDCG@5 0.59
  usage                  n=2    R@5 100.0%   MRR 0.23   nDCG@5 0.60
```

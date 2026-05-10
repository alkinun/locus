```
locus eval

Dataset: evals/locus.synthetic.yaml
Repo: /home/alkinunl/Desktop/locus
Queries: 100
Limit: 10
Embeddings: disabled

Overall:
  Recall@1:   23.0%
  Recall@3:   37.0%
  Recall@5:   46.0%
  Recall@10:  57.0%
  MRR:        0.33
  nDCG@5:     0.36
  nDCG@10:    0.39
  p50:        83 ms
  p95:        138 ms
  max:        153 ms

By style:                                                           AgentTask              n=13   R@5  53.8%   MRR 0.30   nDCG@5 0.34                                                                   Architecture           n=3    R@5   0.0%   MRR 0.05   nDCG@5 0.00                                                                   CasualVague            n=12   R@5  33.3%   MRR 0.15   nDCG@5 0.19                                                                   ChangeTarget           n=14   R@5  42.9%   MRR 0.24   nDCG@5 0.29                                                                   ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00                                                                   DebuggingSymptom       n=18   R@5  55.6%   MRR 0.47   nDCG@5 0.49                                                                   DefinitionQuestion     n=6    R@5   0.0%   MRR 0.02   nDCG@5 0.00                                                                   FuzzyImplementation    n=14   R@5  42.9%   MRR 0.40   nDCG@5 0.38                                                                   TestFinding            n=10   R@5  80.0%   MRR 0.76   nDCG@5 0.76                                                                   UsageQuestion          n=9    R@5  55.6%   MRR 0.27   nDCG@5 0.34

By intent:                                                          architecture           n=1    R@5   0.0%   MRR 0.14   nDCG@5 0.00                                                                   debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00                                                                   implementation         n=97   R@5  45.4%   MRR 0.33   nDCG@5 0.35                                                                   usage                  n=1    R@5 100.0%   MRR 0.20   nDCG@5 0.39

Worst failures:
  synthetic_0001 [Architecture]                                       query: what data is stored in the repository vocabulary structure?
    expected:                                                           - src/repo_meta.rs symbol=RepoVocabulary kind=struct chunk_id=67eec4bedc1eabd8 relevance=3
    top results:                                                        1. codebase.md markdown_section Repository metadata score=34.49                                                                     2. codebase.md markdown_section What this repository is score=32.22                                                                 3. src/repo_meta.rs function build_metadata score=27.56           4. codebase.md markdown_section What `index_repo` does score=27.54                                                                  5. src/repo_meta.rs test repo_vocabulary_collects_core_terms score=26.67                                                            6. src/repo_meta.rs function expand_with_repo_metadata score=25.30
      7. codebase.md markdown_section `RepoMetadata` score=24.44        8. codebase.md markdown_section Query expansion with repo metadata score=23.88
      9. src/repo_meta.rs struct RepoMetadata score=23.46               10. src/evalgen.rs function generate_eval_dataset score=22.64
  synthetic_0002 [CasualVague]
    query: how are the code chunks randomly selected for sampling?
    expected:                                                           - src/evalgen.rs symbol=sample_chunks kind=function chunk_id=259f7f90384731ce relevance=3
    top results:                                                        1. codebase.md markdown_section Sampling heuristics score=27.98
      2. src/chunker.rs function markdown_chunks score=26.78
      3. codebase.md markdown_section `CodeChunk` score=26.21           4. src/chunker.rs test chunks_with_line_ranges_and_overlap score=26.10
      5. src/search.rs function load_chunks_from_index score=25.84
      6. src/chunker.rs function fallback_line_chunks score=25.22
      7. src/search.rs test test_chunks_are_boosted_for_test_queries score=24.28                                                          8. codebase.md markdown_section Build an index score=24.26        9. src/lib.rs test syntax_chunks_improve_search_precision score=23.86                                                               10. src/chunker.rs test large_syntax_chunks_are_split_safely score=22.57
  synthetic_0009 [Architecture]                                       query: what data is stored for a search result displayed in the TUI?
    expected:                                                           - src/tui.rs symbol=VisibleResult kind=struct chunk_id=1937966ecfb61b37 relevance=3
    top results:                                                        1. codebase.md markdown_section What this repository is score=29.74
      2. src/evalgen.rs function generate_eval_dataset score=27.69
      3. src/tui.rs test flattens_grouped_results_in_display_order score=27.40
      4. src/search.rs function search_repo score=26.48                 5. src/main.rs function main score=26.43                          6. src/repo_meta.rs function read_metadata score=25.00
      7. src/tui.rs impl impl VisibleResult score=24.96
      8. src/search.rs struct SearchSession score=24.62                 9. src/search.rs function search score=23.98                      10. src/model.rs function into_result score=23.89             synthetic_0011 [FuzzyImplementation]                                query: how is a string added to a vector only if it doesn't already exist?                                                          expected:                                                           - src/query.rs symbol=push_unique kind=function chunk_id=c436dbe083586d8f relevance=3                                             top results:                                                        1. src/search.rs function tantivy_query_string score=20.94
      2. codebase.md markdown_section Current behavior limits and assumptions score=19.79
      3. codebase.md markdown_section Existing eval datasets in this repo score=18.07                                                     4. src/evalgen.rs function prompt_for_chunk score=17.65           5. src/eval.rs module tests score=14.60                           6. codebase.md markdown_section Behavior score=13.76              7. src/search.rs function document_to_chunk score=13.31           8. codebase.md markdown_section Synthetic eval generation score=11.81                                                               9. codebase.md markdown_section Explanation strings score=11.55                                                                     10. src/search.rs function comment_or_blank_ratio score=11.27                                                                   synthetic_0013 [FuzzyImplementation]
    query: how are search queries processed to identify important and downweighted terms?                                               expected:
      - src/query.rs symbol=analyze_query kind=function chunk_id=f86980c40c548757 relevance=3                                           top results:
      1. src/query.rs test short_plain_terms_are_downweighted score=32.58                                                                 2. src/query.rs test code_like_terms_are_preserved score=25.76
      3. src/repo_meta.rs module tests score=25.36                      4. src/search.rs test rerank_does_not_reward_filler_terms score=24.61
      5. codebase.md markdown_section Search from CLI score=24.13
      6. src/repo_meta.rs test identifier_terms_handles_pascal_case score=23.97
      7. src/repo_meta.rs test identifier_terms_handles_camel_case score=23.94
      8. src/repo_meta.rs test identifier_terms_handles_snake_case score=23.90                                                            9. src/repo_meta.rs test identifier_terms_handles_paths score=23.85                                                                 10. codebase.md markdown_section Important integration tests in `src/lib.rs` score=23.72                                        synthetic_0015 [DebuggingSymptom]                                   query: why are common words like 'what' and 'are' being downweighted in queries?                                                    expected:                                                           - src/query.rs symbol=stopwords_are_downweighted kind=test chunk_id=ee41ae3539a943df relevance=3                                  top results:                                                        1. codebase.md markdown_section Stopword handling score=22.16
      2. src/query.rs test short_plain_terms_are_downweighted score=21.57                                                                 3. codebase.md markdown_section What `index_repo` does score=18.80                                                                  4. codebase.md markdown_section What this repository is score=18.61                                                                 5. src/query.rs test code_like_terms_are_preserved score=18.50
      6. src/query.rs module tests score=18.29                          7. codebase.md markdown_section Code-like term preservation score=18.19
      8. src/search.rs test test_chunks_are_boosted_for_test_queries score=17.27                                                          9. src/query.rs function is_code_like score=15.72
      10. src/query.rs function analyze_query_with_symbols score=15.44                                                                synthetic_0018 [DefinitionQuestion]                                 query: What are the possible Action variants defined in the TUI module?                                                             expected:                                                           - src/tui.rs symbol=Action kind=enum chunk_id=63f2f228f0aee129 relevance=3                                                        top results:                                                        1. codebase.md markdown_section What this repository is score=21.88                                                                 2. codebase.md markdown_section How the main modules depend on each other score=21.84
      3. codebase.md markdown_section What `index_repo` does score=21.75                                                                  4. codebase.md markdown_section Important integration tests in `src/lib.rs` score=18.14                                             5. codebase.md markdown_section Tests score=15.85                 6. codebase.md markdown_section Synthetic eval generation score=15.22                                                               7. codebase.md markdown_section Chunking pipeline score=14.95                                                                       8. codebase.md markdown_section Current behavior limits and assumptions score=14.36                                                 9. codebase.md markdown_section Sampling heuristics score=13.80                                                                     10. codebase.md markdown_section Output formatting score=13.65                                                                  synthetic_0019 [TestFinding]                                        query: which query styles are considered compatible with test chunks?                                                               expected:                                                           - src/evalgen.rs symbol=style_selection_uses_compatible_styles kind=test chunk_id=f967d4a987d32d06 relevance=3                    top results:                                                        1. codebase.md markdown_section Query expansion with repo metadata score=35.90                                                      2. src/repo_meta.rs module tests score=35.22                      3. src/query.rs function analyze_query_with_symbols score=32.34                                                                     4. src/evalgen.rs module tests score=31.70                        5. src/query.rs module tests score=30.83                          6. src/chunker.rs test chunks_with_line_ranges_and_overlap score=30.67                                                              7. src/search.rs test test_chunks_are_boosted_for_test_queries score=30.66                                                          8. codebase.md markdown_section Query styles score=30.30          9. src/lib.rs module integration_tests score=30.20                10. src/evalgen.rs function compatible_style score=29.87      synthetic_0022 [DebuggingSymptom]                                   query: why are function chunks being boosted for implementation queries?                                                            expected:                                                           - src/search.rs symbol=structural_reason kind=function chunk_id=091efb13a5132fb8 relevance=3
    top results:                                                        1. src/search.rs test test_chunks_are_boosted_for_test_queries score=42.94                                                          2. src/output.rs test grouping_keeps_implementation_chunks_primary score=36.88                                                      3. src/search.rs test implementation_phrasing_does_not_trigger_function_boost score=35.67                                           4. src/evalgen.rs test validation_deduplicates_queries_case_insensitively score=29.42                                               5. src/search.rs module tests score=28.78
      6. src/lib.rs test syntax_chunks_improve_search_precision score=27.21                                                               7. codebase.md markdown_section Scoring logic score=27.03
      8. src/chunker.rs test large_syntax_chunks_are_split_safely score=26.70                                                             9. src/chunker.rs test rust_syntax_chunking_extracts_complete_function score=26.59                                                  10. src/search.rs function load_indexed_chunks score=25.24    synthetic_0025 [ChangeTarget]                                       query: how are search terms formatted into a quoted comma separated string?                                                         expected:                                                           - src/search.rs symbol=quote_list kind=function chunk_id=8b6f38093fb7c5b5 relevance=3                                             top results:      1. src/search.rs impl impl SearchSession score=35.55
      2. src/repo_meta.rs module tests score=33.14
      3. src/search.rs function search score=32.56
      4. src/search.rs function tantivy_query_string score=29.57
      5. src/search.rs function search_repo score=28.12
      6. src/lib.rs test indexes_and_searches_fake_repo score=26.75
      7. src/lib.rs test syntax_chunks_improve_search_precision score=26.71
      8. src/model.rs function into_result score=26.54
      9. src/repo_meta.rs function chunk_terms score=26.50
      10. src/repo_meta.rs test repo_vocabulary_collects_core_terms score=26.14
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
  Recall@1:   29.0%
  Recall@3:   43.0%
  Recall@5:   53.0%
  Recall@10:  60.0%
  MRR:        0.39
  nDCG@5:     0.42
  nDCG@10:    0.44
  p50:        145 ms
  p95:        202 ms
  max:        219 ms

By style:
  AgentTask              n=13   R@5  69.2%   MRR 0.35   nDCG@5 0.43
  Architecture           n=3    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  CasualVague            n=12   R@5  33.3%   MRR 0.23   nDCG@5 0.24
  ChangeTarget           n=14   R@5  57.1%   MRR 0.28   nDCG@5 0.37
  ConfigFinding          n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  DebuggingSymptom       n=18   R@5  61.1%   MRR 0.55   nDCG@5 0.56
  DefinitionQuestion     n=6    R@5  33.3%   MRR 0.33   nDCG@5 0.33
  FuzzyImplementation    n=14   R@5  42.9%   MRR 0.40   nDCG@5 0.39
  TestFinding            n=10   R@5  80.0%   MRR 0.76   nDCG@5 0.76
  UsageQuestion          n=9    R@5  55.6%   MRR 0.27   nDCG@5 0.34

By intent:
  architecture           n=1    R@5   0.0%   MRR 0.00   nDCG@5 0.00
  debugging              n=1    R@5 100.0%   MRR 1.00   nDCG@5 1.00
  implementation         n=97   R@5  52.6%   MRR 0.39   nDCG@5 0.42
  usage                  n=1    R@5 100.0%   MRR 0.20   nDCG@5 0.39

Worst failures:
  synthetic_0001 [Architecture]
    query: what data is stored in the repository vocabulary structure?
    expected:
      - src/repo_meta.rs symbol=RepoVocabulary kind=struct chunk_id=67eec4bedc1eabd8 relevance=3
    top results:
      1. codebase.md markdown_section Repository metadata score=36.37
      2. codebase.md markdown_section What this repository is score=32.22
      3. src/repo_meta.rs function build_metadata score=28.93
      4. src/repo_meta.rs test repo_vocabulary_collects_core_terms score=28.57
      5. codebase.md markdown_section What `index_repo` does score=27.54
      6. codebase.md markdown_section `RepoMetadata` score=26.20
      7. src/repo_meta.rs function expand_with_repo_metadata score=25.30
      8. src/repo_meta.rs struct RepoMetadata score=25.30
      9. codebase.md markdown_section Query expansion with repo metadata score=23.88
      10. src/repo_meta.rs module tests score=22.68
  synthetic_0009 [Architecture]
    query: what data is stored for a search result displayed in the TUI?
    expected:
      - src/tui.rs symbol=VisibleResult kind=struct chunk_id=1937966ecfb61b37 relevance=3
    top results:
      1. codebase.md markdown_section What this repository is score=29.74
      2. codebase.md markdown_section `SearchResult` score=27.99
      3. src/evalgen.rs function generate_eval_dataset score=27.69
      4. src/tui.rs test flattens_grouped_results_in_display_order score=27.40
      5. src/search.rs function search_repo score=26.48
      6. src/main.rs function main score=26.43
      7. src/model.rs struct SearchResult score=26.35
      8. src/model.rs function into_result score=25.29
      9. src/search.rs function search score=25.20
      10. src/repo_meta.rs function read_metadata score=25.00
  synthetic_0011 [FuzzyImplementation]
    query: how is a string added to a vector only if it doesn't already exist?
    expected:
      - src/query.rs symbol=push_unique kind=function chunk_id=c436dbe083586d8f relevance=3
    top results:
      1. src/search.rs function string_value score=22.52
      2. src/search.rs function tantivy_query_string score=22.41
      3. src/embeddings.rs impl impl EmbeddingStore score=19.64
      4. codebase.md markdown_section Current behavior limits and assumptions score=19.39
      5. src/evalgen.rs function prompt_for_chunk score=17.65
      6. src/embeddings.rs function search score=13.72
      7. src/search.rs function document_to_chunk score=13.31
      8. src/evalgen.rs test validation_accepts_exact_symbol_for_literal_symbol score=13.14
      9. src/query.rs function detect_intent score=12.68
      10. src/evalgen.rs test validation_deduplicates_queries_case_insensitively score=12.50
  synthetic_0013 [FuzzyImplementation]
    query: how are search queries processed to identify important and downweighted terms?
    expected:
      - src/query.rs symbol=analyze_query kind=function chunk_id=f86980c40c548757 relevance=3
    top results:
      1. src/query.rs test short_plain_terms_are_downweighted score=34.54
      2. src/query.rs test code_like_terms_are_preserved score=27.30
      3. src/repo_meta.rs module tests score=25.36
      4. src/search.rs function query_terms score=24.76
      5. src/search.rs test rerank_does_not_reward_filler_terms score=24.61
      6. codebase.md markdown_section Search from CLI score=24.13
      7. src/search.rs function search score=24.11
      8. src/query.rs module tests score=24.10
      9. src/repo_meta.rs test identifier_terms_handles_pascal_case score=23.97
      10. src/repo_meta.rs test identifier_terms_handles_camel_case score=23.94
  synthetic_0015 [DebuggingSymptom]
    query: why are common words like 'what' and 'are' being downweighted in queries?
    expected:
      - src/query.rs symbol=stopwords_are_downweighted kind=test chunk_id=ee41ae3539a943df relevance=3
    top results:
      1. codebase.md markdown_section Stopword handling score=24.07
      2. src/query.rs test short_plain_terms_are_downweighted score=23.53
      3. src/query.rs test code_like_terms_are_preserved score=20.08
      4. codebase.md markdown_section Code-like term preservation score=20.07
      5. src/query.rs module tests score=19.71
      6. codebase.md markdown_section What this repository is score=19.01
      7. codebase.md markdown_section What `index_repo` does score=17.70
      8. src/search.rs test test_chunks_are_boosted_for_test_queries score=17.27
      9. src/query.rs function analyze_query_with_symbols score=17.08
      10. src/query.rs function is_code_like score=17.05
  synthetic_0018 [DefinitionQuestion]
    query: What are the possible Action variants defined in the TUI module?
    expected:
      - src/tui.rs symbol=Action kind=enum chunk_id=63f2f228f0aee129 relevance=3
    top results:
      1. codebase.md markdown_section What this repository is score=21.88
      2. codebase.md markdown_section How the main modules depend on each other score=21.84
      3. codebase.md markdown_section What `index_repo` does score=21.75
      4. codebase.md markdown_section Important integration tests in `src/lib.rs` score=18.14
      5. codebase.md markdown_section Tests score=15.85
      6. codebase.md markdown_section Synthetic eval generation score=15.22
      7. codebase.md markdown_section Chunking pipeline score=14.95
      8. codebase.md markdown_section Current behavior limits and assumptions score=14.36
      9. codebase.md markdown_section Sampling heuristics score=13.80
      10. codebase.md markdown_section Output formatting score=13.65
  synthetic_0019 [TestFinding]
    query: which query styles are considered compatible with test chunks?
    expected:
      - src/evalgen.rs symbol=style_selection_uses_compatible_styles kind=test chunk_id=f967d4a987d32d06 relevance=3
    top results:
      1. codebase.md markdown_section Query expansion with repo metadata score=35.90
      2. src/repo_meta.rs module tests score=35.22
      3. src/evalgen.rs module tests score=33.58
      4. src/query.rs module tests score=32.52
      5. src/search.rs test test_chunks_are_boosted_for_test_queries score=32.43
      6. src/query.rs function analyze_query_with_symbols score=32.34
      7. codebase.md markdown_section Query styles score=32.12
      8. src/evalgen.rs function compatible_style score=31.77
      9. src/output.rs module tests score=30.96
      10. src/lib.rs module integration_tests score=30.91
  synthetic_0022 [DebuggingSymptom]
    query: why are function chunks being boosted for implementation queries?
    expected:
      - src/search.rs symbol=structural_reason kind=function chunk_id=091efb13a5132fb8 relevance=3
    top results:
      1. src/search.rs test test_chunks_are_boosted_for_test_queries score=44.87
      2. src/output.rs test grouping_keeps_implementation_chunks_primary score=38.61
      3. src/search.rs test implementation_phrasing_does_not_trigger_function_boost score=37.57
      4. src/search.rs module tests score=30.60
      5. src/evalgen.rs test validation_deduplicates_queries_case_insensitively score=29.42
      6. src/lib.rs test syntax_chunks_improve_search_precision score=29.09
      7. src/chunker.rs test large_syntax_chunks_are_split_safely score=28.67
      8. src/chunker.rs test rust_syntax_chunking_extracts_complete_function score=28.26
      9. codebase.md markdown_section Scoring logic score=27.03
      10. src/chunker.rs test rust_test_function_is_marked_test score=26.72
  synthetic_0025 [ChangeTarget]
    query: how are search terms formatted into a quoted comma separated string?
    expected:
      - src/search.rs symbol=quote_list kind=function chunk_id=8b6f38093fb7c5b5 relevance=3
    top results:
      1. src/search.rs impl impl SearchSession score=35.55
      2. src/repo_meta.rs module tests score=33.14
      3. src/search.rs function search score=32.96
      4. src/search.rs function tantivy_query_string score=31.07
      5. src/search.rs function search_repo score=28.12
      6. src/repo_meta.rs function chunk_terms score=27.96
      7. src/search.rs function query_terms score=27.67
      8. src/repo_meta.rs function identifier_terms score=27.58
      9. src/query.rs test code_like_terms_are_preserved score=27.54
      10. src/query.rs function split_terms score=27.23
  synthetic_0027 [DefinitionQuestion]
    query: What is the definition of the SymbolReference struct?
    expected:
      - src/repo_meta.rs symbol=SymbolReference kind=struct chunk_id=d71dbc6c216c4807 relevance=3
    top results:
      1. codebase.md markdown_section `SymbolGraph` score=22.72
      2. src/repo_meta.rs struct SymbolGraph score=21.53
      3. src/repo_meta.rs function build_references score=20.74
      4. src/repo_meta.rs test symbol_graph_creates_reference_edges score=18.62
      5. src/repo_meta.rs function build_metadata score=16.80
      6. codebase.md markdown_section Query expansion with repo metadata score=15.90
      7. src/evalgen.rs function as_str score=15.55
      8. src/repo_meta.rs struct SymbolDefinition score=14.61
      9. src/evalgen.rs function allows_exact_symbol score=14.15
      10. src/search.rs function structural_reason score=13.57
```

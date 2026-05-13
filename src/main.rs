use anyhow::Result;
use clap::Parser;
use locus::cli::{Cli, Command};
use locus::eval::{EvalOptions, print_human_report, run_eval};
use locus::evalgen::{GenerateEvalOptions, generate_eval_dataset};
use locus::indexer::index_repo;
use locus::output::{
    group_ranked_results, print_human_grouped_results, print_human_results, print_index_summary,
    print_json_grouped_results, print_json_results,
};
use locus::reranker::download_reranker_model;
use locus::search::{SearchOptions, search_repo_with_options};
use locus::tui::run_tui;

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Command::Index {
                path,
                download_embedding,
                download_reranker,
            } => {
                if download_reranker {
                    eprintln!("Downloading jina-reranker-v1-turbo-en if needed...");
                    download_reranker_model()?;
                }
                let summary = index_repo(&path, download_embedding)?;
                print_index_summary(&summary);
            }
            Command::Search {
                query,
                path,
                limit,
                json,
                grouped,
                rerank,
                rerank_limit,
            } => {
                let summary = search_repo_with_options(
                    &path,
                    &query,
                    limit,
                    SearchOptions {
                        use_embeddings: true,
                        use_reranker: rerank,
                        rerank_limit,
                    },
                )?;
                if json && grouped {
                    let grouped_results = group_ranked_results(&summary.results);
                    print_json_grouped_results(grouped_results)?;
                } else if json {
                    let results = summary
                        .results
                        .into_iter()
                        .enumerate()
                        .map(|(idx, ranked)| ranked.into_result(idx + 1))
                        .collect();
                    print_json_results(results)?;
                } else if grouped {
                    print_human_grouped_results(&summary.results, summary.elapsed.as_millis());
                } else {
                    print_human_results(&summary.results, summary.elapsed.as_millis());
                }
            }
            Command::GenerateEval {
                path,
                out,
                count,
                endpoint,
                model,
                seed,
                concurrency,
            } => {
                let summary = generate_eval_dataset(GenerateEvalOptions {
                    path,
                    out,
                    count,
                    endpoint,
                    model,
                    seed,
                    concurrency,
                })?;
                println!("Generated {} eval items", summary.generated);
                println!("Skipped {}", summary.skipped);
                println!("Wrote {}", summary.out.display());
                if !summary.style_counts.is_empty() {
                    println!("Style counts:");
                    for (style, count) in summary.style_counts {
                        println!("  {}: {}", style.as_str(), count);
                    }
                }
                if !summary.skip_reasons.is_empty() {
                    println!("Skip reasons:");
                    for (reason, count) in summary.skip_reasons {
                        println!("  {}: {}", reason, count);
                        if let Some(example) = summary.skip_examples.get(&reason) {
                            println!("    example: {}", example.replace('\n', " | "));
                        }
                    }
                }
            }
            Command::Eval {
                path,
                dataset,
                limit,
                embedding,
                no_embedding,
                rerank,
                rerank_limit,
                json,
                failures,
            } => {
                let report = run_eval(EvalOptions {
                    path,
                    dataset,
                    limit,
                    use_embeddings: embedding && !no_embedding,
                    use_reranker: rerank,
                    rerank_limit,
                    failures,
                })?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_human_report(&report);
                }
            }
        }
    } else {
        run_tui(cli.path)?;
    }

    Ok(())
}

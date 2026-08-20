use std::collections::VecDeque;

use super::PipelineRunner;
use super::PipelineError;
use crate::action::StepError;
use crate::pipeline::Pipeline;

impl PipelineRunner {
    /// Topologically sort pipeline steps based on dependencies (Kahn's algorithm)
    /// Returns indices in execution order, or error if cycle is detected or dependency is missing.
    pub fn topological_sort(&self, pipeline: &Pipeline) -> Result<Vec<usize>, PipelineError> {
        let n = pipeline.steps.len();

        // Build a map from step name to index
        let name_to_idx: std::collections::HashMap<&str, usize> = pipeline
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.as_str(), i))
            .collect();

        // Initialize in-degree and adjacency list
        let mut in_degree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

        // Build dependency graph
        for (i, step) in pipeline.steps.iter().enumerate() {
            for dep_name in &step.dependencies {
                match name_to_idx.get(dep_name.as_str()) {
                    Some(&dep_idx) => {
                        // Edge from dep_idx to i (i depends on dep_idx)
                        adj[dep_idx].push(i);
                        in_degree[i] += 1;
                    }
                    None => {
                        return Err(PipelineError::StepFailed {
                            step: step.name.clone(),
                            error: StepError::ActionFailed {
                                reason: format!("Unknown dependency: {}", dep_name),
                            },
                        });
                    }
                }
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::new();

        while let Some(idx) = queue.pop_front() {
            order.push(idx);
            for &next in &adj[idx] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push_back(next);
                }
            }
        }

        // Check for cycles
        if order.len() != n {
            return Err(PipelineError::StepFailed {
                step: "DAG".into(),
                error: StepError::ActionFailed {
                    reason: "Cycle detected in step dependencies".into(),
                },
            });
        }

        Ok(order)
    }
}

// Phase 10: 全ジョブ実行

use crate::pipeline::job_runner::{JobConfig, JobResult, run_job};

/// Run multiple jobs, collecting results.
/// One job failure does NOT prevent other jobs from running.
#[allow(dead_code)]
pub fn run_all_jobs(jobs: &[JobConfig]) -> Vec<crate::error::Result<JobResult>> {
    jobs.iter().map(run_job).collect()
}

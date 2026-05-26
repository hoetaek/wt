use anyhow::Result;
use std::sync::mpsc;
use std::thread;

pub(crate) enum ParallelControl {
    Continue,
    Stop,
}

pub(crate) struct ParallelCompletion<T, R> {
    pub(crate) item: T,
    pub(crate) result: R,
}

pub(crate) struct ParallelRunOutcome<T> {
    pub(crate) pending: Vec<T>,
}

pub(crate) fn run_bounded_parallel<T, R, Start, Work, Complete>(
    items: Vec<T>,
    jobs: usize,
    mut on_start: Start,
    work: Work,
    mut on_complete: Complete,
) -> Result<ParallelRunOutcome<T>>
where
    T: Clone + Send,
    R: Send,
    Start: FnMut(&T) -> Result<()>,
    Work: Fn(T) -> R + Sync,
    Complete: FnMut(ParallelCompletion<T, R>) -> Result<ParallelControl>,
{
    let worker_count = jobs.max(1);
    let (tx, rx) = mpsc::channel::<ParallelCompletion<T, R>>();
    let mut next = 0;
    let mut active = 0;
    let mut stopped = false;

    thread::scope(|scope| -> Result<()> {
        loop {
            while !stopped && active < worker_count && next < items.len() {
                let item = items[next].clone();
                on_start(&item)?;
                let worker_item = item.clone();
                let tx = tx.clone();
                let work = &work;
                scope.spawn(move || {
                    let result = work(worker_item);
                    let _ = tx.send(ParallelCompletion { item, result });
                });
                active += 1;
                next += 1;
            }

            if active == 0 {
                break;
            }

            let completion = rx
                .recv()
                .map_err(|_| anyhow::anyhow!("Parallel worker result channel closed"))?;
            active -= 1;
            if matches!(on_complete(completion)?, ParallelControl::Stop) {
                stopped = true;
            }
        }
        Ok(())
    })?;

    let pending = if stopped {
        items.into_iter().skip(next).collect()
    } else {
        Vec::new()
    };
    Ok(ParallelRunOutcome { pending })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_parallel_runs_all_items() {
        let mut results = Vec::new();

        let outcome = run_bounded_parallel(
            vec![1, 2, 3],
            2,
            |_| Ok(()),
            |item| item * 2,
            |completion| {
                results.push((completion.item, completion.result));
                Ok(ParallelControl::Continue)
            },
        )
        .unwrap();

        results.sort();
        assert_eq!(results, vec![(1, 2), (2, 4), (3, 6)]);
        assert!(outcome.pending.is_empty());
    }

    #[test]
    fn bounded_parallel_reports_not_started_items_after_stop() {
        let mut results = Vec::new();

        let outcome = run_bounded_parallel(
            vec![1, 2, 3],
            1,
            |_| Ok(()),
            |item| item * 2,
            |completion| {
                results.push((completion.item, completion.result));
                Ok(ParallelControl::Stop)
            },
        )
        .unwrap();

        assert_eq!(results, vec![(1, 2)]);
        assert_eq!(outcome.pending, vec![2, 3]);
    }
}

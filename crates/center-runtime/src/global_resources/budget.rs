//! Request-scoped resource budgets for GlobalResources inventory scans.
//!
//! A budget is shared by every concurrent scan participating in one inventory
//! request. Upstream page attempts are consumed permanently, while retained
//! items and bytes are provisional until a namespace scan succeeds.

use std::sync::{Arc, Mutex, MutexGuard};

/// The independently enforced dimensions of an inventory budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDimension {
    Items,
    RetainedBytes,
    UpstreamPages,
}

impl std::fmt::Display for BudgetDimension {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Items => "items",
            Self::RetainedBytes => "retained bytes",
            Self::UpstreamPages => "upstream pages",
        };
        formatter.write_str(name)
    }
}

/// Invalid limits supplied when constructing an [`InventoryBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidInventoryBudget {
    pub dimension: BudgetDimension,
}

impl std::fmt::Display for InvalidInventoryBudget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "inventory budget limit for {} must be greater than zero",
            self.dimension
        )
    }
}

impl std::error::Error for InvalidInventoryBudget {}

/// A failed reservation, including the dimension that would exceed its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetExceeded {
    pub dimension: BudgetDimension,
    pub limit: usize,
    pub used: usize,
    pub requested: usize,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "inventory budget for {} exceeded: limit {}, used {}, requested {}",
            self.dimension, self.limit, self.used, self.requested
        )
    }
}

impl std::error::Error for BudgetExceeded {}

/// Configured request-level limits for an inventory scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryBudgetLimits {
    pub max_items: usize,
    pub max_retained_bytes: usize,
    pub max_upstream_pages: usize,
}

/// Current usage of an inventory budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InventoryBudgetUsage {
    pub items: usize,
    pub retained_bytes: usize,
    pub upstream_pages: usize,
}

struct InventoryBudgetInner {
    limits: InventoryBudgetLimits,
    usage: Mutex<InventoryBudgetUsage>,
}

/// A thread-safe budget shared by all scans in one inventory request.
///
/// Clones refer to the same usage counters. Create a new budget for every
/// top-level inventory request; committed usage intentionally remains charged
/// for the lifetime of that request.
#[derive(Clone)]
pub struct InventoryBudget {
    inner: Arc<InventoryBudgetInner>,
}

impl InventoryBudget {
    pub fn new(limits: InventoryBudgetLimits) -> Result<Self, InvalidInventoryBudget> {
        for (dimension, limit) in [
            (BudgetDimension::Items, limits.max_items),
            (BudgetDimension::RetainedBytes, limits.max_retained_bytes),
            (BudgetDimension::UpstreamPages, limits.max_upstream_pages),
        ] {
            if limit == 0 {
                return Err(InvalidInventoryBudget { dimension });
            }
        }

        Ok(Self {
            inner: Arc::new(InventoryBudgetInner {
                limits,
                usage: Mutex::new(InventoryBudgetUsage::default()),
            }),
        })
    }

    #[cfg(test)]
    pub fn usage(&self) -> InventoryBudgetUsage {
        *self.lock_usage()
    }

    /// Permanently consumes one upstream page attempt.
    ///
    /// This charge is intentionally irreversible because a failed request or
    /// namespace still consumed upstream work.
    pub fn try_consume_page(&self) -> Result<(), BudgetExceeded> {
        let mut usage = self.lock_usage();
        usage.upstream_pages = checked_reservation(
            BudgetDimension::UpstreamPages,
            self.inner.limits.max_upstream_pages,
            usage.upstream_pages,
            1,
        )?;
        Ok(())
    }

    /// Atomically reserves retained items and bytes for one successful page.
    ///
    /// The returned reservation rolls back automatically unless
    /// [`PayloadReservation::commit`] is called.
    pub fn try_reserve_payload(
        &self,
        items: usize,
        retained_bytes: usize,
    ) -> Result<PayloadReservation, BudgetExceeded> {
        let mut usage = self.lock_usage();
        let next_items = checked_reservation(
            BudgetDimension::Items,
            self.inner.limits.max_items,
            usage.items,
            items,
        )?;
        let next_retained_bytes = checked_reservation(
            BudgetDimension::RetainedBytes,
            self.inner.limits.max_retained_bytes,
            usage.retained_bytes,
            retained_bytes,
        )?;

        usage.items = next_items;
        usage.retained_bytes = next_retained_bytes;
        drop(usage);

        Ok(PayloadReservation {
            budget: self.clone(),
            usage: InventoryBudgetUsage {
                items,
                retained_bytes,
                upstream_pages: 0,
            },
            active: true,
        })
    }

    fn release(&self, released: InventoryBudgetUsage) {
        let mut usage = self.lock_usage();
        usage.items = usage
            .items
            .checked_sub(released.items)
            .expect("reservation item usage must remain accounted");
        usage.retained_bytes = usage
            .retained_bytes
            .checked_sub(released.retained_bytes)
            .expect("reservation byte usage must remain accounted");
        usage.upstream_pages = usage
            .upstream_pages
            .checked_sub(released.upstream_pages)
            .expect("reservation page usage must remain accounted");
    }

    fn lock_usage(&self) -> MutexGuard<'_, InventoryBudgetUsage> {
        self.inner
            .usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn checked_reservation(
    dimension: BudgetDimension,
    limit: usize,
    used: usize,
    requested: usize,
) -> Result<usize, BudgetExceeded> {
    used.checked_add(requested)
        .filter(|next| *next <= limit)
        .ok_or(BudgetExceeded {
            dimension,
            limit,
            used,
            requested,
        })
}

/// A provisional retained-payload charge against an [`InventoryBudget`].
///
/// Dropping or explicitly rolling back a reservation returns its item and byte
/// charges. Committing keeps them charged.
#[must_use = "payload reservations must be committed or rolled back"]
pub struct PayloadReservation {
    budget: InventoryBudget,
    usage: InventoryBudgetUsage,
    active: bool,
}

impl PayloadReservation {
    pub fn commit(mut self) {
        self.active = false;
    }

    #[cfg(test)]
    pub fn rollback(mut self) {
        self.release();
    }

    fn release(&mut self) {
        if self.active {
            self.budget.release(self.usage);
            self.active = false;
        }
    }
}

impl Drop for PayloadReservation {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn budget(limits: InventoryBudgetLimits) -> InventoryBudget {
        InventoryBudget::new(limits).expect("test limits must be valid")
    }

    #[test]
    fn rejects_zero_limits_by_dimension() {
        for (limits, dimension) in [
            (
                InventoryBudgetLimits {
                    max_items: 0,
                    max_retained_bytes: 1,
                    max_upstream_pages: 1,
                },
                BudgetDimension::Items,
            ),
            (
                InventoryBudgetLimits {
                    max_items: 1,
                    max_retained_bytes: 0,
                    max_upstream_pages: 1,
                },
                BudgetDimension::RetainedBytes,
            ),
            (
                InventoryBudgetLimits {
                    max_items: 1,
                    max_retained_bytes: 1,
                    max_upstream_pages: 0,
                },
                BudgetDimension::UpstreamPages,
            ),
        ] {
            assert_eq!(
                InventoryBudget::new(limits)
                    .err()
                    .expect("zero limit must fail"),
                InvalidInventoryBudget { dimension }
            );
        }
    }

    #[test]
    fn accepts_exact_boundaries() {
        let budget = budget(InventoryBudgetLimits {
            max_items: 3,
            max_retained_bytes: 12,
            max_upstream_pages: 1,
        });

        budget.try_consume_page().expect("page must fit");
        budget
            .try_reserve_payload(3, 12)
            .expect("exact limits must be accepted")
            .commit();

        assert_eq!(
            budget.usage(),
            InventoryBudgetUsage {
                items: 3,
                retained_bytes: 12,
                upstream_pages: 1,
            }
        );
    }

    #[test]
    fn failed_reservation_does_not_partially_charge() {
        let budget = budget(InventoryBudgetLimits {
            max_items: 10,
            max_retained_bytes: 10,
            max_upstream_pages: 2,
        });
        budget
            .try_reserve_payload(2, 2)
            .expect("first page must fit")
            .commit();
        let before = budget.usage();

        assert_eq!(
            budget
                .try_reserve_payload(3, 9)
                .err()
                .expect("byte budget must reject the page"),
            BudgetExceeded {
                dimension: BudgetDimension::RetainedBytes,
                limit: 10,
                used: 2,
                requested: 9,
            }
        );
        assert_eq!(budget.usage(), before);
    }

    #[test]
    fn reports_each_exceeded_dimension() {
        let cases = [
            (
                InventoryBudgetLimits {
                    max_items: 1,
                    max_retained_bytes: 100,
                    max_upstream_pages: 10,
                },
                2,
                1,
                BudgetDimension::Items,
            ),
            (
                InventoryBudgetLimits {
                    max_items: 100,
                    max_retained_bytes: 1,
                    max_upstream_pages: 10,
                },
                1,
                2,
                BudgetDimension::RetainedBytes,
            ),
        ];

        for (limits, items, bytes, expected_dimension) in cases {
            let budget = budget(limits);
            let error = budget
                .try_reserve_payload(items, bytes)
                .err()
                .expect("reservation must exceed its configured dimension");
            assert_eq!(error.dimension, expected_dimension);
            assert_eq!(budget.usage(), InventoryBudgetUsage::default());
        }

        let budget = budget(InventoryBudgetLimits {
            max_items: 100,
            max_retained_bytes: 100,
            max_upstream_pages: 1,
        });
        budget.try_consume_page().expect("first page must fit");
        let error = budget
            .try_consume_page()
            .expect_err("second page must exceed page budget");
        assert_eq!(error.dimension, BudgetDimension::UpstreamPages);
    }

    #[test]
    fn explicit_rollback_restores_all_dimensions() {
        let budget = budget(InventoryBudgetLimits {
            max_items: 3,
            max_retained_bytes: 12,
            max_upstream_pages: 1,
        });
        let reservation = budget
            .try_reserve_payload(3, 12)
            .expect("reservation must fit");

        reservation.rollback();

        assert_eq!(budget.usage(), InventoryBudgetUsage::default());
        budget
            .try_reserve_payload(3, 12)
            .expect("rolled back capacity must be reusable")
            .commit();
    }

    #[test]
    fn drop_rolls_back_uncommitted_reservation() {
        let budget = budget(InventoryBudgetLimits {
            max_items: 3,
            max_retained_bytes: 12,
            max_upstream_pages: 1,
        });

        {
            let _reservation = budget
                .try_reserve_payload(3, 12)
                .expect("reservation must fit");
        }

        assert_eq!(budget.usage(), InventoryBudgetUsage::default());
    }

    #[test]
    fn checked_add_overflow_is_reported_without_charging() {
        let budget = budget(InventoryBudgetLimits {
            max_items: usize::MAX,
            max_retained_bytes: usize::MAX,
            max_upstream_pages: 2,
        });
        budget
            .try_reserve_payload(usize::MAX, 1)
            .expect("first reservation must fit")
            .commit();

        let error = budget
            .try_reserve_payload(1, 1)
            .err()
            .expect("overflow must be rejected");

        assert_eq!(error.dimension, BudgetDimension::Items);
        assert_eq!(
            budget.usage(),
            InventoryBudgetUsage {
                items: usize::MAX,
                retained_bytes: 1,
                upstream_pages: 0,
            }
        );
    }

    #[test]
    fn failed_payload_can_roll_back_without_refunding_page_attempt() {
        let budget = budget(InventoryBudgetLimits {
            max_items: 3,
            max_retained_bytes: 12,
            max_upstream_pages: 2,
        });

        budget.try_consume_page().expect("page must fit");
        let reservation = budget.try_reserve_payload(3, 12).expect("payload must fit");
        drop(reservation);

        assert_eq!(
            budget.usage(),
            InventoryBudgetUsage {
                items: 0,
                retained_bytes: 0,
                upstream_pages: 1,
            }
        );
    }

    #[test]
    fn concurrent_reservations_never_exceed_limits() {
        const WORKERS: usize = 32;
        const LIMIT: usize = 7;

        let budget = budget(InventoryBudgetLimits {
            max_items: LIMIT,
            max_retained_bytes: LIMIT * 10,
            max_upstream_pages: LIMIT,
        });
        let barrier = Arc::new(Barrier::new(WORKERS));
        let mut workers = Vec::with_capacity(WORKERS);

        for _ in 0..WORKERS {
            let budget = budget.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                budget.try_consume_page()?;
                budget.try_reserve_payload(1, 10).map(|reservation| {
                    reservation.commit();
                })
            }));
        }

        let successes = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker must not panic"))
            .filter(Result::is_ok)
            .count();

        assert_eq!(successes, LIMIT);
        assert_eq!(
            budget.usage(),
            InventoryBudgetUsage {
                items: LIMIT,
                retained_bytes: LIMIT * 10,
                upstream_pages: LIMIT,
            }
        );
    }

    #[test]
    fn concurrent_rollbacks_restore_capacity() {
        const WORKERS: usize = 16;

        let budget = budget(InventoryBudgetLimits {
            max_items: WORKERS,
            max_retained_bytes: WORKERS,
            max_upstream_pages: WORKERS,
        });
        let barrier = Arc::new(Barrier::new(WORKERS));
        let mut workers = Vec::with_capacity(WORKERS);

        for _ in 0..WORKERS {
            let budget = budget.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let reservation = budget
                    .try_reserve_payload(1, 1)
                    .expect("all workers must fit");
                barrier.wait();
                reservation.rollback();
            }));
        }

        for worker in workers {
            worker.join().expect("worker must not panic");
        }

        assert_eq!(budget.usage(), InventoryBudgetUsage::default());
    }
}

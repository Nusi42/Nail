# nail-portability

Nail's importers, exporters, backup archives and diagnostics bundle (plan
M4c WP121; contract ADR 0094). Pure file work: readers yield messages and
PIM items with per-item issues, writers take them; the core
(`nail-core/src/portability.rs`) runs the jobs, writes into the store /
operation queues and reports progress.

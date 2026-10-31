# Security

## Reporting a problem

If you find a security problem in Nail, please report it privately, not in a
public issue:

1. Open the [Security tab](https://github.com/Nusi42/Nail/security) of this
   repository.
2. Choose **Report a vulnerability** and describe what you found.

Only the maintainer sees the report. Please include what is needed to
understand and repeat the problem: the Nail version or the commit, the steps, what you expected and what happened, and how serious you
think it is. Never attach real mail, contacts or account data; describe them
or use made-up examples.

You will get an answer within a week. Once the problem is fixed, the fix
ships in a release with a short note, and you are named as the finder if you
wish.

## What is covered

Nail is a preview before version 1.0. Fixes go into the newest release and
the `main` branch; older releases are not patched.

Please test only on your own computer and your own accounts, and do not read,
change or delete data that is not yours while you look for a problem.

## How Nail is built for security

The design of HTML mail in a sandbox, remote content, encryption and the
remote UI is in [`docs/design.md`](docs/design.md#security--encryption).

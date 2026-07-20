# IPAM

IPAM (IP Address Management) is the single source of truth for the IP
addressing of your infrastructure.

## What it does

Managing IP addresses with spreadsheets stops working the day two teams
allocate the same range. IPAM solves this by centralising the whole
addressing plan in one service:

- **Inventory** — every network, subnet and address is recorded in one
  place, always up to date.
- **Allocation** — request an address or a range and get one that is
  guaranteed to be free, without conflicts.
- **Visibility** — see at a glance what is used, what is reserved and what
  is available across your whole address space.
- **History** — know when an address was assigned, to what, and by whom.

## Who it is for

Network engineers, platform teams and operators who need a reliable
addressing plan shared across teams, tools and automation.

## How to use it

IPAM runs as a service exposing an HTTP API, designed to be consumed by
humans through tooling and by automation directly. It is cloud-native and
ships with the health probes orchestrators expect.

## Contributing

Everything you need to build, run and evolve the project — tooling,
conventions and rules — lives in [CONTRIBUTING.md](CONTRIBUTING.md).

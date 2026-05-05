# deploy/systemd-example/ — testing & local-deploy scaffold

> **This directory is not part of the supported production deployment
> path.** It exists so a contributor or tester can stand up the daemon
> + Tor under systemd on a Linux box quickly. Do not lift any of this
> into a production environment without reviewing it against your own
> ops requirements (secret management, log aggregation, supervision
> policy, network namespacing, image-based deploys, etc.).

## What's here

| File | Purpose |
|---|---|
| `torpc-tor.service.template` | Systemd unit template for the Tor hidden-service process. Carries a hardening baseline (`NoNewPrivileges`, `ProtectSystem=strict`, `RestrictAddressFamilies`, `MemoryDenyWriteExecute`, etc.). |
| `torpc-daemon.service.template` | Systemd unit template for the Rust daemon (`target/release/torpc`). Wants the Tor unit; depends on `${TORPC_HOME}/.env` for runtime config. |
| `install-systemd.sh` | Substitutes `${USER}` and `${TORPC_HOME}` into both templates and either prints the result or installs to `/etc/systemd/system/` via `sudo`. |

## Why templates instead of static unit files

The original repo shipped a `torpc-tor.service` with hardcoded paths
(`User=random_anon`, `/Users/random_anon/dev/torpc/...`) — useless to
anyone but the original author. Templating lets one source serve any
host while keeping the security-relevant directives in version control.

## Usage

Print rendered units to stdout (review pass, no side effects):

```bash
deploy/systemd-example/install-systemd.sh
```

Render and install to `/etc/systemd/system/` (requires `sudo`):

```bash
deploy/systemd-example/install-systemd.sh --install
```

Override user/home explicitly (e.g. running as a dedicated `torpc` user):

```bash
deploy/systemd-example/install-systemd.sh \
    --user torpc \
    --home /opt/torpc \
    --install
```

After install, enable and start:

```bash
sudo systemctl enable --now torpc-tor torpc-daemon
```

## When this is fine to use

- Local Linux dev boxes where you want graceful start/stop and journal
  log routing without writing your own service files.
- A single-node staging environment for end-to-end manual testing.
- A reference implementation when authoring your own production units.

## When this is **not** fine to use

- Multi-tenant / public-facing deployments. The templates have no
  resource limits beyond the `tower` concurrency cap inside the daemon,
  no log shipping, no rotation policy.
- Anywhere `sudo`-piping into `/etc/systemd/system/` from a script is
  unacceptable (most CI and image-baking pipelines).
- Production deployments where systemd unit files should be built into
  an immutable image rather than rendered on the host.

If your production runs on Kubernetes / nomad / fly.io / similar, ignore
this directory entirely.

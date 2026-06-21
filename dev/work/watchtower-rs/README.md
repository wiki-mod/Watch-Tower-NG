<div align="center">

  # Watch-Tower-NG

  A Rust rewrite of [Watchtower](https://github.com/containrrr/watchtower) — automated Docker container base image updates.

  [![CI](https://github.com/wiki-mod/Watch-Tower-NG/actions/workflows/ci.yml/badge.svg)](https://github.com/wiki-mod/Watch-Tower-NG/actions/workflows/ci.yml)
  [![License](https://img.shields.io/github/license/wiki-mod/Watch-Tower-NG.svg)](https://www.apache.org/licenses/LICENSE-2.0)
  [![Latest Version](https://img.shields.io/github/tag/wiki-mod/Watch-Tower-NG.svg)](https://github.com/wiki-mod/Watch-Tower-NG/releases)
  [![Image](https://ghcr.io/wiki-mod/watchtower-ng)](https://github.com/wiki-mod/Watch-Tower-NG/pkgs/container/watchtower-ng)

</div>

## About

Watch-Tower-NG is a **Rust port** of [Watchtower](https://github.com/containrrr/watchtower), maintaining 1:1 behavioral parity with the original Go implementation while adding the safety guarantees of the Rust type system.

With Watch-Tower-NG you can update the running version of your containerized app simply by pushing a new image to your registry. Watch-Tower-NG will pull down the new image, gracefully shut down the existing container, and restart it with the same options that were used when it was deployed initially.

> **Note:** Watch-Tower-NG is intended for homelabs, media centers, and local dev environments. For production workloads, consider Kubernetes or similar orchestration platforms.

## Quick Start

```bash
docker run --detach \
    --name watchtower \
    --volume /var/run/docker.sock:/var/run/docker.sock \
    ghcr.io/wiki-mod/watchtower-ng
```

Or with docker-compose:

```yaml
services:
  watchtower:
    image: ghcr.io/wiki-mod/watchtower-ng
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
```

## Documentation

Full documentation: see the [docs/](docs/) directory or the project wiki.

## Attribution

Watch-Tower-NG is a derivative work of [Watchtower](https://github.com/containrrr/watchtower) by the Watchtower contributors, licensed under the Apache License 2.0. See [NOTICE](../../NOTICE) for full attribution details.

## License

Apache License 2.0 — see [LICENSE](../../LICENSE).

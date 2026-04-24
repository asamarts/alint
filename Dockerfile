# syntax=docker/dockerfile:1.7
#
# alint's release Docker image. Built by `.github/workflows/release.yml`
# as part of the multi-arch release; pushed to
# `ghcr.io/asamarts/alint:<tag>` + `:latest`.
#
# The image intentionally does NOT build from source — the release
# workflow pre-stages the musl binaries under
# `docker-context/linux-<amd64|arm64>/alint` so the image matches
# the binaries in the GitHub Release byte-for-byte. Local
# `docker build .` therefore fails unless you stage the binaries
# yourself; the image is a release artifact, not a dev tool.
#
# Usage:
#
#   docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest check
#
# Runs as the distroless `nonroot` user (UID 65532). Files on the
# mounted volume must be world-readable; if `alint fix` needs to
# write, pass `-u $(id -u):$(id -g)` to keep host ownership.

FROM gcr.io/distroless/static-debian12:nonroot

ARG TARGETARCH

# The release workflow stages one binary per arch under
# `docker-context/linux-<arch>/alint`. `TARGETARCH` is `amd64` /
# `arm64` for each platform in the buildx matrix.
COPY --chmod=0755 linux-${TARGETARCH}/alint /usr/local/bin/alint

WORKDIR /repo
USER nonroot

# `alint` with no subcommand defaults to `check`, so `CMD ["check"]`
# only matters if a user explicitly overrides it with `--help` etc.
ENTRYPOINT ["/usr/local/bin/alint"]
CMD ["check"]

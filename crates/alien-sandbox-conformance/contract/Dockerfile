# Build from the crate directory: docker build -f contract/Dockerfile .
# Root-supervisor conformance image: the ENTRYPOINT boots as root and drops the
# workload server to uid 1001. The image bakes no agent mode; the backend decides
# how it is started.
FROM ubuntu:24.04
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 util-linux ca-certificates \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && mkdir /tmp/alien-contract && chmod 1777 /tmp/alien-contract
ARG METADATA_BACKEND=""
ENV ALIEN_CONFORMANCE_METADATA=${METADATA_BACKEND}
COPY contract/boot.py contract/server.py contract/census.py contract/probe_metadata.py entrypoint/metadata.py /opt/contract-conformance/
USER 0
ENTRYPOINT ["python3", "/opt/contract-conformance/boot.py"]

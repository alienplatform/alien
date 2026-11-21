FROM public.ecr.aws/docker/library/ubuntu:24.04
# The probes are small Python programs run as commands in the sandbox.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 \
  && rm -rf /var/lib/apt/lists/*

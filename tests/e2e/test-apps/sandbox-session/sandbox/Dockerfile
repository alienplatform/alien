FROM public.ecr.aws/docker/library/ubuntu:24.04
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
ARG SANDBOX_IMAGE_MARK=v1
RUN echo "sandbox-session ${SANDBOX_IMAGE_MARK}" > /etc/sandbox-session-version

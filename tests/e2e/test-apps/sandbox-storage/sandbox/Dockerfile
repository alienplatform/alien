FROM public.ecr.aws/docker/library/ubuntu:24.04
# git and sqlite3 for the integrity checks; coreutils' sync for the writer's per-line fsync.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git sqlite3 \
 && rm -rf /var/lib/apt/lists/*

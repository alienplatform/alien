FROM public.ecr.aws/docker/library/ubuntu:24.04
# The acceptance checks run SQLite, git and Python against the replicated directory.
RUN apt-get update \
 && apt-get install -y --no-install-recommends sqlite3 git python3 ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
RUN echo "sandbox-storage v1" > /etc/sandbox-storage-version

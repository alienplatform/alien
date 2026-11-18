FROM public.ecr.aws/docker/library/ubuntu:24.04
# python3 drives the fixture and the writer; sqlite3 and git check what a restore brings back.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates python3 sqlite3 git \
 && rm -rf /var/lib/apt/lists/*
# Commands run as an ordinary uid that owns the replicated directory, as a coding agent's do.
RUN useradd -u 1001 -m app && mkdir -p /workspace && chown 1001:1001 /workspace
COPY --chmod=0755 populate.py manifest.py writer.py /opt/app/
USER app
CMD ["python3", "/opt/app/writer.py"]

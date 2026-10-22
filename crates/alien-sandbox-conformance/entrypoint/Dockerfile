FROM ubuntu:24.04 AS no-user
RUN apt-get update && apt-get install -y --no-install-recommends python3 util-linux ca-certificates \
    && apt-get clean && mkdir /tmp/alien-entrypoint && chmod 1777 /tmp/alien-entrypoint
COPY supervisor.py inspect.py /opt/entrypoint-conformance/
ENTRYPOINT ["python3", "/opt/entrypoint-conformance/supervisor.py"]

FROM no-user AS root
USER 0

FROM no-user AS nonroot
USER 2001:2001

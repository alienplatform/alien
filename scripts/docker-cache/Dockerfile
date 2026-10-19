FROM alpine:3.21@sha256:ce64758a109eb420d874a118f87920e625e12d3634e03b4a5573fd9f6e5d3507
ARG DEP_VERSION=1
COPY dependency.txt /dependency.txt
RUN mkdir /dependencies && awk 'BEGIN { for(i=0;i<300000;i++) print i, i*i }' > /dependencies/table && sha256sum /dependencies/table > /dependencies/checksum && date +%s > /dependencies/built-at && head -c 16 /dev/urandom | od -An -tx1 > /dependencies/build-nonce && echo "$DEP_VERSION" > /dependencies/version
COPY app.txt /app.txt
CMD ["sh", "-c", "cat /app.txt /dependency.txt /dependencies/version; sha256sum -c /dependencies/checksum"]

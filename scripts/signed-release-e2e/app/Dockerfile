FROM busybox:1.37
ARG MESSAGE=release-1
# No RUN step, so the image builds for any platform without emulation.
ENV MESSAGE=$MESSAGE
CMD ["sh", "-c", "echo started: $MESSAGE; env | sort; exec sleep 2147483647"]

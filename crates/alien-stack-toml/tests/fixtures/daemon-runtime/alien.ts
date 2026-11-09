import * as alien from "@alienplatform/core"

function daemon(id: string, image: string, runtime: alien.DaemonRuntime) {
  return new alien.Daemon(id).code({ type: "image", image }).runtime(runtime).build()
}

const hostNetwork = daemon("host-network", "ghcr.io/acme/net:1.0.0", { networkMode: "host" })
const hostPid = daemon("host-pid", "ghcr.io/acme/pid:1.0.0", { pidNamespace: "host" })
const privileged = daemon("privileged", "ghcr.io/acme/privileged:1.0.0", { privileged: true })
const unprivileged = daemon("unprivileged", "ghcr.io/acme/unprivileged:1.0.0", {
  privileged: false,
  user: "1000:1000",
})
const mounts = daemon("mounts", "ghcr.io/acme/mounts:1.0.0", {
  mounts: [
    { source: "/var/run/docker.sock", target: "/var/run/docker.sock" },
    { source: "/proc", target: "/host/proc", options: "ro" },
  ],
})

export default new alien.Stack("acme-runtime")
  .platforms(["machines"])
  .add(hostNetwork, "live")
  .add(hostPid, "live")
  .add(privileged, "live")
  .add(unprivileged, "live")
  .add(mounts, "live")
  .build()

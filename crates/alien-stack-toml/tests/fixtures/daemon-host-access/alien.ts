import * as alien from "@alienplatform/core"

const installer = new alien.Daemon("installer")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "installer" } })
  .runtime({
    privileged: true,
    pidNamespace: "host",
    networkMode: "host",
    mounts: [{ source: "/", target: "/host" }],
    user: "0",
  })
  .build()

const plain = new alien.Daemon("plain")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "plain" } })
  .build()

export default new alien.Stack("acme-host")
  .platforms(["machines"])
  .add(installer, "live")
  .add(plain, "live")
  .build()

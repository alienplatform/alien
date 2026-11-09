import * as alien from "@alienplatform/core"

const volumeKey = new alien.Key("volume-key").build()

const defaultVolume = new alien.Container("default-volume")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("50Gi")
  .permissions("default-volume")
  .build()

const customMount = new alien.Container("custom-mount")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("200Gi", { mountPath: "/var/lib/db" })
  .permissions("custom-mount")
  .build()

const cacheVolume = new alien.Container("cache-volume")
  .code({ type: "image", image: "ghcr.io/acme/cache:1.0.0" })
  .cpu(1)
  .memory("2Gi")
  .persistentStorage("20Gi", { backups: false })
  .permissions("cache-volume")
  .build()

const explicitBackups = new alien.Container("explicit-backups")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("20Gi", { backups: {} })
  .permissions("explicit-backups")
  .build()

const hourlyBackups = new alien.Container("hourly-backups")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("20Gi", { backups: { enabled: true, intervalHours: 1 } })
  .permissions("hourly-backups")
  .build()

const longRetention = new alien.Container("long-retention")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("20Gi", { backups: { retentionDays: 90 } })
  .permissions("long-retention")
  .build()

const encrypted = new alien.Container("encrypted")
  .code({ type: "image", image: "ghcr.io/acme/db:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .persistentStorage("20Gi", {
    encryptionKey: volumeKey,
    backups: { intervalHours: 12, retentionDays: 30 },
  })
  .permissions("encrypted")
  .build()

export default new alien.Stack("acme-volumes")
  .platforms(["aws", "gcp", "azure"])
  .add(volumeKey, "frozen")
  .add(defaultVolume, "live")
  .add(customMount, "live")
  .add(cacheVolume, "live")
  .add(explicitBackups, "live")
  .add(hourlyBackups, "live")
  .add(longRetention, "live")
  .add(encrypted, "live")
  .permissions({
    profiles: {
      "default-volume": {},
      "custom-mount": {},
      "cache-volume": {},
      "explicit-backups": {},
      "hourly-backups": {},
      "long-retention": {},
      encrypted: {},
    },
  })
  .build()

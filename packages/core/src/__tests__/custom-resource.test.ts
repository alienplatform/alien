import { describe, expect, it } from "vitest"
import * as z from "zod"

import * as alien from "../index.js"

// What `alien extensions build` writes into an extension's generated module: a builder per
// type, extending CustomResourceType the way Alien's own builders extend theirs, and the
// extension as the default export.
const DnsZoneSchema = z.object({ id: z.string(), domain: z.string() })

class DnsZone extends alien.CustomResourceType {
  private _config: Partial<z.infer<typeof DnsZoneSchema>> = {}

  constructor(id: string) {
    super()
    this._config.id = id
  }

  public domain(domain: string): this {
    this._config.domain = domain
    return this
  }

  public build(): alien.Resource {
    return this.resource({ type: "acme/dns-zone", ...DnsZoneSchema.parse(this._config) })
  }
}

const acme: alien.ExtensionManifest = {
  name: "acme",
  version: "0.1.0",
  digest: "sha256:4f2a",
  resourceTypes: {
    "acme/dns-zone": {
      lifecycles: ["frozen", "live"],
      holdsData: false,
      setup: { cloudformation: ["aws"], terraform: ["aws"], helm: [], importers: ["aws"] },
      controllerPlatforms: ["aws"],
      permissionSets: [
        {
          id: "acme/dns-zone/records-write",
          description: "Write the zone's DNS records",
          platforms: {
            aws: [
              {
                grant: { actions: ["route53:ChangeResourceRecordSets"] },
                binding: { resource: { resources: ["arn:aws:route53:::hostedzone/*"] } },
              },
            ],
          },
        },
      ],
      link: { permissionSets: ["acme/dns-zone/records-write"] },
      schemas: {
        config: { type: "object", required: ["id", "domain"] },
        outputs: { type: "object", required: ["zoneId"] },
      },
    },
  },
}

class TypedAs extends alien.CustomResourceType {
  constructor(private readonly type: string) {
    super()
  }

  public build(): alien.Resource {
    return this.resource({ type: this.type, id: "zone" })
  }
}

describe("custom resource types", () => {
  it("adds a generated builder's resource to the stack with the extension that defines it", () => {
    const zone = new DnsZone("zone").domain("internal.acme.dev").build()
    const stack = new alien.Stack("my-app").extensions([acme]).add(zone, "frozen").build()

    expect(stack.resources.zone?.config).toEqual({
      type: "acme/dns-zone",
      id: "zone",
      domain: "internal.acme.dev",
    })
    expect(stack.resources.zone?.lifecycle).toBe("frozen")
    expect(stack.extensions).toEqual([acme])
    expect(alien.StackSchema.parse(JSON.parse(JSON.stringify(stack)))).toEqual(stack)
  })

  it("keeps extensions in the order the stack lists them", () => {
    const backups: alien.ExtensionManifest = {
      name: "backups",
      version: "1.0.0",
      digest: "sha256:9c1e",
      resourceTypes: {},
    }
    const stack = new alien.Stack("my-app").extensions([backups, acme]).build()
    expect(stack.extensions?.map(extension => extension.name)).toEqual(["backups", "acme"])
  })

  it("leaves a stack with no extensions without the field", () => {
    const stack = new alien.Stack("my-app")
      .add(new alien.Storage("files").build(), "frozen")
      .build()
    expect(stack).not.toHaveProperty("extensions")
  })

  it("links a custom resource to a worker like a built-in one", () => {
    const zone = new DnsZone("zone").domain("internal.acme.dev").build()
    const api = new alien.Worker("api")
      .code({ type: "image", image: "docker.io/library/rust:latest" })
      .link(zone)
      .permissions("api")
      .build()
    const stack = new alien.Stack("my-app")
      .extensions([acme])
      .add(zone, "frozen")
      .add(api, "live")
      .permissions({ profiles: { api: { zone: ["acme/dns-zone/records-write"] } } })
      .build()

    expect(stack.resources.api?.config.links).toEqual([{ type: "acme/dns-zone", id: "zone" }])
  })

  it("carries the gate and prose into the stack entry", () => {
    const stackInputs = alien.inputs({
      privateDns: alien.boolean({
        providedBy: "deployer",
        required: true,
        label: "Private DNS",
        description: "Whether the deployment creates the private DNS zone.",
      }),
    })
    const zone = new DnsZone("zone")
      .domain("internal.acme.dev")
      .enabled(stackInputs.privateDns)
      .displayName("Private DNS")
      .description("Names the workers resolve inside the network.")
      .build()
    const entry = new alien.Stack("my-app")
      .inputs(stackInputs)
      .extensions([acme])
      .add(zone, "live")
      .build().resources.zone

    expect(entry?.enabledWhen).toBe("privateDns")
    expect(entry?.displayName).toBe("Private DNS")
    expect(entry?.description).toBe("Names the workers resolve inside the network.")
  })

  it.each([
    "dns-zone",
    "storage",
    "/dns-zone",
    "acme/",
    "alien/dns-zone",
    "alien/dns/zone",
    "experimental/aws-opensearch",
  ])("refuses the type name '%s'", type => {
    expect(() => new TypedAs(type).build()).toThrow(
      `Resource type '${type}' must be named '<extension>/<type>', such as 'acme/dns-zone'; the extension names 'alien' and 'experimental' are reserved`,
    )
  })

  it("accepts a slash in the type's own name, as Alien's core does", () => {
    expect(new TypedAs("acme/dns/zone").build().config.type).toBe("acme/dns/zone")
  })
})

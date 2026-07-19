import { type Email as EmailConfig, EmailSchema, type ResourceType } from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  EmailOutputs,
  EmailDomainOutputs,
  EmailDkimToken,
  EmailInbound,
  EmailEvents,
  Email as EmailConfig,
} from "./generated/index.js"
export { EmailSchema as EmailConfigSchema } from "./generated/index.js"

/**
 * Email infrastructure for sending and receiving mail on customer-owned
 * domains (AWS SES). Provisions one email identity per domain (Easy DKIM) and
 * a shared configuration set, plus optional inbound and event wiring.
 *
 * After deployment, create the per-domain DKIM CNAME records from the
 * resource outputs — SES verifies each domain once the records exist in DNS.
 *
 * When inbound mail is configured, the provisioned SES receipt rule set must
 * be activated manually after deployment (only one receipt rule set can be
 * active per AWS account, and CloudFormation cannot activate one):
 * `aws ses set-active-receipt-rule-set --rule-set-name <ruleSetName>`.
 */
export class Email {
  private _config: Partial<EmailConfig> = {
    domains: [],
  }

  /**
   * Creates a new Email builder.
   * @param id Identifier for the email resource. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any email resource.
   * Used for creating permission targets that apply to all email resources.
   * @returns The "email" resource type.
   */
  public static any(): ResourceType {
    return "email"
  }

  /**
   * Sets the mail domains this resource sends (and optionally receives) on.
   * At least one domain is required.
   * @param domains The mail domains (e.g. `["mail.example.com"]`).
   * @returns The Email builder instance.
   */
  public domains(domains: string[]): this {
    this._config.domains = domains
    return this
  }

  /**
   * Adds a single mail domain.
   * @param domain The mail domain (e.g. `"mail.example.com"`).
   * @returns The Email builder instance.
   */
  public domain(domain: string): this {
    this._config.domains = [...(this._config.domains ?? []), domain]
    return this
  }

  /**
   * Configures inbound mail: raw incoming mail for the configured domains is
   * written to the linked Storage bucket. Note the post-deploy activation
   * step described in the class documentation.
   * @param storage The Storage resource that receives raw incoming mail.
   * @returns The Email builder instance.
   */
  public inbound(storage: Resource): this {
    this._config.inbound = { storage: storage.ref() }
    return this
  }

  /**
   * Configures sending events: send / delivery / bounce / complaint /
   * delivery-delay / reject events are delivered to the linked Queue.
   * @param queue The Queue resource that receives sending events.
   * @returns The Email builder instance.
   */
  public events(queue: Resource): this {
    this._config.events = { queue: queue.ref() }
    return this
  }

  /**
   * Builds and validates the email configuration.
   * @returns An immutable Resource representing the configured email infrastructure.
   * @throws Error if the email configuration is invalid.
   */
  public build(): Resource {
    const config = EmailSchema.parse(this._config)
    if (config.domains.length === 0) {
      throw new Error(`Email resource '${config.id}' must configure at least one domain`)
    }

    return new Resource({
      type: "email",
      ...config,
    })
  }
}

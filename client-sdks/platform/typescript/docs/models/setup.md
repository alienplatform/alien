# Setup


## Supported Types

### `models.SetupCloudformation`

```typescript
const value: models.SetupCloudformation = {
  method: "cloudformation",
  deploymentPortalUrl: "https://ashamed-bakeware.org",
  launchUrl: "https://flawless-offset.com/",
  templateUrl: "https://lavish-punctuation.name",
  stackName: "<value>",
  stackSettings: {},
};
```

### `models.SetupGoogleOauth`

```typescript
const value: models.SetupGoogleOauth = {
  method: "google-oauth",
  deploymentPortalUrl: "https://prime-skyscraper.org/",
  managerUrl: "https://worldly-issue.biz/",
  oauthStartUrl: "https://winding-icebreaker.com",
  region: "<value>",
  stackSettings: {},
};
```

### `models.SetupTerraform`

```typescript
const value: models.SetupTerraform = {
  method: "terraform",
  deploymentPortalUrl: "https://close-ghost.net/",
  managerUrl: "https://untried-accelerator.biz",
  providerSource: "<value>",
  moduleSource: "<value>",
  moduleInputs: {
    "key": "<value>",
    "key1": "<value>",
  },
  mainTf: "<value>",
  tfvars: "<value>",
  commands: "<value>",
  stackSettings: {},
};
```


# ManagerRetryResponse


## Supported Types

### `models.ManagerRetryResponseSetup`

```typescript
const value: models.ManagerRetryResponseSetup = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  setupStatus: "pending",
  setupToken: "<value>",
  setupTokenId: "<id>",
  deploymentLink: "<value>",
  setupConfig: {
    metadata: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    policy: {
      allowedPlatforms: [],
      allowedSetupMethods: [
        "google-oauth",
      ],
    },
    environmentVariables: [
      {
        name: "<value>",
        type: "secret",
        targetResources: null,
      },
    ],
  },
  setup: {
    method: "terraform",
    deploymentPortalUrl: "https://confused-majority.net/",
    managerUrl: "https://deadly-cruelty.org/",
    providerSource: "<value>",
    moduleSource: "<value>",
    moduleInputs: {},
    mainTf: "<value>",
    tfvars: "<value>",
    commands: "<value>",
    stackSettings: {},
  },
  mode: "setup",
};
```

### `models.ManagerRetryDeploymentResponse`

```typescript
const value: models.ManagerRetryDeploymentResponse = {
  mode: "deployment",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  setupStatus: "provisioning",
  deploymentId: "<id>",
  message: "<value>",
};
```


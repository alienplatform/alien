# WhoamiResponse

Whoami endpoint response — the authenticated subject plus onboarding flags (user only).


## Supported Types

### `models.WhoamiResponseUser`

```typescript
const value: models.WhoamiResponseUser = {
  kind: "user",
  id: "<id>",
  email: "Hector93@hotmail.com",
  workspaceId: "<id>",
  role: "workspace.member",
  cliConnected: true,
  onboardingDismissedAt: new Date("2024-04-20T09:35:24.669Z"),
};
```

### `models.ServiceAccountSubject`

```typescript
const value: models.ServiceAccountSubject = {
  kind: "serviceAccount",
  id: "<id>",
  workspaceId: "<id>",
  scope: {
    type: "deployment",
    deploymentId: "<id>",
    projectId: "<id>",
  },
  role: "workspace.member",
};
```


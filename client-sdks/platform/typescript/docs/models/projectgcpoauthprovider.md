# ProjectGcpOAuthProvider


## Supported Types

### `models.ProjectGcpOAuthProviderAlienManaged`

```typescript
const value: models.ProjectGcpOAuthProviderAlienManaged = {
  mode: "alien-managed",
  redirectUris: [
    "https://strident-technologist.org/",
    "https://inferior-plastic.name/",
    "https://neat-planula.name/",
  ],
};
```

### `models.ProjectGcpOAuthProviderCustom`

```typescript
const value: models.ProjectGcpOAuthProviderCustom = {
  mode: "custom",
  clientId: "1234567890-abc123.apps.googleusercontent.com",
  hasClientSecret: false,
  redirectUris: [
    "https://rowdy-catalyst.biz/",
    "https://inborn-gymnast.name/",
  ],
};
```


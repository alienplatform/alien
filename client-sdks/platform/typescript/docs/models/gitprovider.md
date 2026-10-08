# GitProvider

Provider-specific repository information, resolved server-side from remoteUrl


## Supported Types

### `models.GitHubProvider`

```typescript
const value: models.GitHubProvider = {
  type: "github",
  org: "<value>",
  repo: "<value>",
};
```

### `models.GitLabProvider`

```typescript
const value: models.GitLabProvider = {
  type: "gitlab",
  namespace: "<value>",
  project: "<value>",
};
```

### `string`

```typescript
const value: string = "<value>";
```

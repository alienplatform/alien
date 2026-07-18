# AwsCredentials

Supported AWS authentication methods


## Supported Types

### `models.AwsCredentialsAccessKeys`

```typescript
const value: models.AwsCredentialsAccessKeys = {
  accessKeyId: "<id>",
  secretAccessKey: "<value>",
  type: "accessKeys",
};
```

### `models.AwsCredentialsSessionCredentials`

```typescript
const value: models.AwsCredentialsSessionCredentials = {
  accessKeyId: "<id>",
  expiresAt: "1748259840067",
  secretAccessKey: "<value>",
  sessionToken: "<value>",
  type: "sessionCredentials",
};
```

### `models.AwsCredentialsImds`

```typescript
const value: models.AwsCredentialsImds = {
  type: "imds",
};
```

### `models.AwsCredentialsProfile`

```typescript
const value: models.AwsCredentialsProfile = {
  name: "<value>",
  type: "profile",
};
```

### `models.AwsCredentialsWebIdentity`

```typescript
const value: models.AwsCredentialsWebIdentity = {
  config: {
    roleArn: "<value>",
    webIdentityTokenFile: "<value>",
  },
  type: "webIdentity",
};
```


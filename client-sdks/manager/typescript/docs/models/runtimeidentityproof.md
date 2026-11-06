# RuntimeIdentityProof

Proof material is intentionally not Debug: providers must never log it.


## Supported Types

### `models.RuntimeIdentityProofAws`

```typescript
const value: models.RuntimeIdentityProofAws = {
  presignedUrl: "https://lumbering-clavicle.com",
  type: "aws",
};
```

### `models.RuntimeIdentityProofGcp`

```typescript
const value: models.RuntimeIdentityProofGcp = {
  idToken: "<value>",
  type: "gcp",
};
```

### `models.RuntimeIdentityProofAzure`

```typescript
const value: models.RuntimeIdentityProofAzure = {
  accessToken: "<value>",
  type: "azure",
};
```

### `models.RuntimeIdentityProofSession`

```typescript
const value: models.RuntimeIdentityProofSession = {
  token: "<value>",
  type: "session",
};
```

### `models.RuntimeIdentityProofDailyKey`

```typescript
const value: models.RuntimeIdentityProofDailyKey = {
  key: "<key>",
  type: "dailyKey",
};
```

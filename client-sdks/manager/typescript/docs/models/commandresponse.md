# CommandResponse

Command response from deployment


## Supported Types

### `models.CommandResponseSuccess`

```typescript
const value: models.CommandResponseSuccess = {
  response: {
    inlineBase64: "<value>",
    mode: "inline",
  },
  status: "success",
};
```

### `models.CommandResponseError`

```typescript
const value: models.CommandResponseError = {
  code: "<value>",
  message: "<value>",
  status: "error",
};
```


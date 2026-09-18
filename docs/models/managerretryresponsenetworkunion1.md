# ManagerRetryResponseNetworkUnion1


## Supported Types

### `models.ManagerRetryResponseNetworkUseDefault1`

```typescript
const value: models.ManagerRetryResponseNetworkUseDefault1 = {
  type: "use-default",
};
```

### `models.ManagerRetryResponseNetworkCreate1`

```typescript
const value: models.ManagerRetryResponseNetworkCreate1 = {
  type: "create",
};
```

### `models.ManagerRetryResponseNetworkByoVpcAws1`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcAws1 = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.ManagerRetryResponseNetworkByoVpcGcp1`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcGcp1 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.ManagerRetryResponseNetworkByoVnetAzure1`

```typescript
const value: models.ManagerRetryResponseNetworkByoVnetAzure1 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```


terraform {
  cloud {
    organization = "alienplatform"
    workspaces { name = "alien-test-infra" }
  }
}

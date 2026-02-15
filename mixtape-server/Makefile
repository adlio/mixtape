# Makefile for deploying a mixtape agent to AWS Bedrock AgentCore.
#
# This replaces the manual docker/aws CLI workflow with composable targets
# backed by a CloudFormation template for infrastructure.
#
# Usage:
#   make deploy STACK_NAME=my-agent                  # full deploy
#   make deploy STACK_NAME=my-agent BINARY=my_agent  # custom binary
#   make local                                       # run locally
#   make clean  STACK_NAME=my-agent                  # tear down
#
# Prerequisites:
#   - AWS CLI v2 with configured credentials
#   - Docker with buildx support
#
# Targets:
#   deploy   - full pipeline: infra → build → push → runtime
#   infra    - deploy CloudFormation stack (ECR repo + IAM role)
#   build    - build ARM64 Docker image
#   push     - authenticate with ECR and push image
#   runtime  - create or update the AgentCore runtime
#   local    - build and run the container locally
#   status   - show current deployment status
#   clean    - delete AgentCore runtime and CloudFormation stack

# ── Configuration ────────────────────────────────────────────────────
STACK_NAME ?= mixtape-agent
BINARY     ?= agentcore_server
REGION     ?= $(shell aws configure get region 2>/dev/null || echo us-west-2)
TEMPLATE   := agentcore-stack.yaml
DOCKERFILE := Dockerfile.agentcore

# Docker build context is the workspace root (one level up)
CONTEXT    := ..

# ── Derived values ───────────────────────────────────────────────────
ACCOUNT_ID  = $(shell aws sts get-caller-identity --query Account --output text)
ECR_URI     = $(shell aws cloudformation describe-stacks \
                --stack-name $(STACK_NAME) --region $(REGION) \
                --query "Stacks[0].Outputs[?OutputKey=='EcrRepositoryUri'].OutputValue" \
                --output text 2>/dev/null)
ROLE_ARN    = $(shell aws cloudformation describe-stacks \
                --stack-name $(STACK_NAME) --region $(REGION) \
                --query "Stacks[0].Outputs[?OutputKey=='AgentCoreRoleArn'].OutputValue" \
                --output text 2>/dev/null)

.PHONY: deploy infra build push runtime local status clean help

# ── Primary targets ──────────────────────────────────────────────────

## Deploy everything: infra → build → push → runtime
deploy: infra build push runtime
	@echo ""
	@echo "  Deployment complete: $(STACK_NAME)"
	@echo "  Region: $(REGION)"
	@echo ""

## Deploy CloudFormation stack (ECR repository + IAM role)
infra:
	@echo "▸ Deploying infrastructure stack: $(STACK_NAME)..."
	@aws cloudformation deploy \
		--template-file $(TEMPLATE) \
		--stack-name $(STACK_NAME) \
		--parameter-overrides AgentName=$(STACK_NAME) \
		--capabilities CAPABILITY_NAMED_IAM \
		--region $(REGION) \
		--no-fail-on-empty-changeset
	@echo "  ✓ Infrastructure ready"

## Build ARM64 Docker image
build:
	@echo "▸ Building ARM64 image..."
	@docker buildx build --platform linux/arm64 \
		-f $(DOCKERFILE) \
		--build-arg BINARY_NAME=$(BINARY) \
		-t $(STACK_NAME):latest \
		--load $(CONTEXT)
	@echo "  ✓ Image built: $(STACK_NAME):latest"

## Push image to ECR
push:
	@echo "▸ Pushing image to ECR..."
	@aws ecr get-login-password --region $(REGION) | \
		docker login --username AWS \
		--password-stdin $(ACCOUNT_ID).dkr.ecr.$(REGION).amazonaws.com 2>/dev/null
	@docker tag $(STACK_NAME):latest $(ECR_URI):latest
	@docker push $(ECR_URI):latest
	@echo "  ✓ Image pushed: $(ECR_URI):latest"

## Create or update the AgentCore runtime
runtime:
	@EXISTING=$$(aws bedrock-agentcore-control list-agent-runtimes \
		--region $(REGION) \
		--query "agentRuntimeSummaries[?agentRuntimeName=='$(STACK_NAME)'].agentRuntimeArn | [0]" \
		--output text 2>/dev/null); \
	if [ -n "$$EXISTING" ] && [ "$$EXISTING" != "None" ]; then \
		echo "▸ Updating AgentCore runtime..."; \
		aws bedrock-agentcore-control update-agent-runtime \
			--agent-runtime-id "$$EXISTING" \
			--agent-runtime-artifact \
				"{\"containerConfiguration\":{\"containerUri\":\"$(ECR_URI):latest\"}}" \
			--region $(REGION) > /dev/null; \
		echo "  ✓ Runtime updated: $$EXISTING"; \
	else \
		echo "▸ Creating AgentCore runtime..."; \
		RESULT=$$(aws bedrock-agentcore-control create-agent-runtime \
			--agent-runtime-name $(STACK_NAME) \
			--agent-runtime-artifact \
				"{\"containerConfiguration\":{\"containerUri\":\"$(ECR_URI):latest\"}}" \
			--network-configuration '{"networkMode":"PUBLIC"}' \
			--role-arn $(ROLE_ARN) \
			--region $(REGION) \
			--output json); \
		ARN=$$(echo "$$RESULT" | grep -o '"agentRuntimeArn":"[^"]*"' | cut -d'"' -f4); \
		echo "  ✓ Runtime created: $$ARN"; \
	fi

# ── Utility targets ─────────────────────────────────────────────────

## Build and run container locally
local:
	@echo "▸ Building image for local testing..."
	@docker build \
		-f $(DOCKERFILE) \
		--build-arg BINARY_NAME=$(BINARY) \
		-t $(STACK_NAME) $(CONTEXT)
	@echo ""
	@echo "  Health check: curl http://localhost:8080/ping"
	@echo "  Invoke:       curl -N -X POST http://localhost:8080/invocations \\"
	@echo "                  -H 'Content-Type: application/json' \\"
	@echo "                  -d '{\"prompt\": \"Hello\"}'"
	@echo ""
	@docker run --rm -p 8080:8080 \
		-e AWS_REGION=$(REGION) \
		-e AWS_ACCESS_KEY_ID=$${AWS_ACCESS_KEY_ID:-} \
		-e AWS_SECRET_ACCESS_KEY=$${AWS_SECRET_ACCESS_KEY:-} \
		-e AWS_SESSION_TOKEN=$${AWS_SESSION_TOKEN:-} \
		$(STACK_NAME)

## Show current deployment status
status:
	@echo "Stack:   $(STACK_NAME)"
	@echo "Region:  $(REGION)"
	@echo ""
	@echo "CloudFormation:"
	@aws cloudformation describe-stacks --stack-name $(STACK_NAME) --region $(REGION) \
		--query "Stacks[0].{Status:StackStatus,Created:CreationTime,Updated:LastUpdatedTime}" \
		--output table 2>/dev/null || echo "  (no stack found)"
	@echo ""
	@echo "AgentCore Runtime:"
	@aws bedrock-agentcore-control list-agent-runtimes --region $(REGION) \
		--query "agentRuntimeSummaries[?agentRuntimeName=='$(STACK_NAME)'] | [0]" \
		--output table 2>/dev/null || echo "  (no runtime found)"

## Delete AgentCore runtime and CloudFormation stack
clean:
	@echo "▸ Cleaning up $(STACK_NAME)..."
	@EXISTING=$$(aws bedrock-agentcore-control list-agent-runtimes \
		--region $(REGION) \
		--query "agentRuntimeSummaries[?agentRuntimeName=='$(STACK_NAME)'].agentRuntimeArn | [0]" \
		--output text 2>/dev/null); \
	if [ -n "$$EXISTING" ] && [ "$$EXISTING" != "None" ]; then \
		echo "  Deleting AgentCore runtime..."; \
		aws bedrock-agentcore-control delete-agent-runtime \
			--agent-runtime-id "$$EXISTING" --region $(REGION) 2>/dev/null || true; \
	fi
	@echo "  Deleting CloudFormation stack..."
	@aws cloudformation delete-stack --stack-name $(STACK_NAME) --region $(REGION)
	@aws cloudformation wait stack-delete-complete --stack-name $(STACK_NAME) --region $(REGION)
	@echo "  ✓ Cleanup complete"

## Show this help
help:
	@echo "mixtape AgentCore Deployment"
	@echo ""
	@echo "Usage: make <target> [STACK_NAME=name] [BINARY=binary] [REGION=region]"
	@echo ""
	@echo "Targets:"
	@echo "  deploy   Full pipeline: infra → build → push → runtime"
	@echo "  infra    Deploy CloudFormation stack (ECR + IAM role)"
	@echo "  build    Build ARM64 Docker image"
	@echo "  push     Push image to ECR"
	@echo "  runtime  Create or update AgentCore runtime"
	@echo "  local    Build and run container locally"
	@echo "  status   Show current deployment status"
	@echo "  clean    Delete runtime and CloudFormation stack"
	@echo "  help     Show this help"
	@echo ""
	@echo "Variables:"
	@echo "  STACK_NAME  Stack/agent name  (default: mixtape-agent)"
	@echo "  BINARY      Rust binary name  (default: agentcore_server)"
	@echo "  REGION      AWS region        (default: from aws configure)"

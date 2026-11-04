STAMP := $(shell touch /tmp/pwned)
$(shell touch /tmp/pwned-target):
	@true
include $(shell touch /tmp/pwned-include)
safe: ## Safe
	$(shell touch /tmp/pwned-recipe)

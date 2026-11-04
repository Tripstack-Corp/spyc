define RULE
$(1): ## from define
	@echo $(1)
endef
$(foreach t,a b c,$(eval $(call RULE,$(t))))
define EMPTY
endef
real::
	@true
real::
	@true

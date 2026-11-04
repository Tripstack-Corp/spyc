ifeq ($(A),1)
ifdef B
ifndef C
ifneq (x,y)
deep: ## deep
	@true
else
alt:
endif
endif
else ifeq ($(A),2)
two:
endif
else
none:
endif
endif

---
kind: agreement
title: Deed of Sale
respondent_type: person
code: real_estate__deed_of_sale
jurisdiction: NV
confidential: true
questionnaire:
  BEGIN:
    _: END
  END: {}
workflow:
  BEGIN:
    _: lawyer_review
  lawyer_review:
    approved: notarization__pending
  notarization__pending:
    notarized: END
  END: {}
---

# Deed of Sale

This Deed is made between {{client_name}} ("Buyer") and the named Seller for the property described
herein. Choice of law: Nevada. Buyer's signature must be acknowledged by a Nevada notary public under
Nevada's Uniform Law on Notarial Acts (NRS 240.161 to 240.169).

Buyer: ______________________
Date:  ______________________

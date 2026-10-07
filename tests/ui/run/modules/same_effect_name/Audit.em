pub effect Log where
  emit : Int -> Unit

pub audited : Unit -> <Log> Unit
audited () = emit 1

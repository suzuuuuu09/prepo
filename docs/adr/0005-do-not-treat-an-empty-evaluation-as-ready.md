# Do not treat an empty evaluation as Ready

A Full Run with no applicable required checks reports `NotEvaluated`, not `Ready`, while retaining exit code 0 and exposing both total and required evaluated counts in structured output. Configuration must contain at least one check and one required check, but platform applicability can still leave a particular environment without readiness evidence; making that state explicit avoids a misleading Ready result from vacuous truth.

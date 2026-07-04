# A fitted linear-regression model. The constant-weighted linear combination of
# the float features is recognised as an OLS predictor, so the emitted `predict`
# cites C-OLS-MODEL-UNIQUENESS alongside the float-arithmetic contract.
def predict(sqft: float, bedrooms: float, age: float) -> float:
    return 0.115 * sqft + 15.2 * bedrooms + -0.9 * age + 32.5

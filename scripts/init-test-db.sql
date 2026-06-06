-- Create non-privileged test user (RLS will apply)
-- This user has NO special privileges - just basic connection rights
-- NOT a superuser, NOT createdb - RLS policies will be enforced
CREATE USER test_user WITH PASSWORD 'test_pass';

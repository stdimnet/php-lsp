<?php

namespace App;

use App\Models\User;

require_once __DIR__ . '/../lib/helpers.php';

class Service
{
    public function run(User $user): string
    {
        $missing = undefined_function();
        return greet($user->getName());
    }
}

<?php

namespace App\Models;

/**
 * A user of the application.
 */
class User
{
    public function __construct(private string $name)
    {
    }

    /** Returns the display name. */
    public function getName(): string
    {
        return $this->name;
    }
}

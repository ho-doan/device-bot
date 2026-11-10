import { Link } from 'react-router-dom'

export function NotFound() {
  return (
    <div className="center-page">
      <p>This page is outside your orbit.</p>
      <Link className="btn btn-filled" to="/home">
        Back to home
      </Link>
    </div>
  )
}

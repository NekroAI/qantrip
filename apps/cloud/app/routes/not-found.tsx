import { data } from 'react-router'

export const loader = () => {
  throw data('这个页面不存在，或许被施了隐身术。', { status: 404 })
}

export default function NotFound() {
  return null
}
